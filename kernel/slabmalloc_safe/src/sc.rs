//! A SCAllocator that can allocate fixed size objects.

use crate::*;

/// A genius(?) const min()
///
/// # What this does
/// * create an array of the two elements you want to choose between
/// * create an arbitrary boolean expression
/// * cast said expresison to a usize
/// * use that value to index into the array created above
///
/// # Source
/// https://stackoverflow.com/questions/53619695/calculating-maximum-value-of-a-set-of-constant-expressions-at-compile-time
#[cfg(feature = "unstable")]
const fn cmin(a: usize, b: usize) -> usize {
    [a, b][(a > b) as usize]
}

/// The boring variant of min (not const).
#[cfg(not(feature = "unstable"))]
fn cmin(a: usize, b: usize) -> usize {
    core::cmp::min(a, b)
}

/// A slab allocator allocates elements of a fixed size.
///
/// It maintains three internal lists of `MappedPages8k`
/// from which it can allocate memory.
///
///  * `empty_slabs`: Is a list of pages that the SCAllocator maintains, but
///    has 0 allocations in them.
///  * `slabs`: A list of pages partially allocated and still have room for more.
///  * `full_slabs`: A list of pages that are completely allocated.
///
/// On allocation we allocate memory from `slabs`, however if the list is empty
/// we try to reclaim a page from `empty_slabs` before we return with an out-of-memory
/// error. If a page becomes full after the allocation we move it from `slabs` to
/// `full_slabs`.
///
/// Similarly, on dealloaction we might move a page from `full_slabs` to `slabs`
/// or from `slabs` to `empty_slabs` after we deallocated an object.
pub struct SCAllocator {
    /// Maximum possible allocation size for this `SCAllocator`.
    pub(crate) size: usize,
    /// Keeps track of succeeded allocations.
    pub(crate) allocation_count: usize,
    /// Keeps track of `MappedPages8k` in the heap
    pub(crate) page_count: usize,
    /// max objects per page
    pub(crate) obj_per_page: usize,
    /// Keeps track of the empty pages in the heap.
    pub(crate) empty_count: usize, 
    /// List to hold empty MappedPages (nothing allocated in these).
    pub(crate) empty_slabs: Vec<MappedPages8k>, 
    /// List to hold partially used MappedPages (some objects allocated but pages are not full).
    pub(crate) slabs: Vec<MappedPages8k>, 
    /// List to hold full MappedPages (everything allocated in these don't need to search them).
    pub(crate) full_slabs: Vec<MappedPages8k>, 
}

/// Creates an instance of a scallocator, we do this in a macro because we
/// re-use the code in const and non-const functions
macro_rules! new_sc_allocator {
    ($size:expr) => {
        SCAllocator {
            size: $size,
            allocation_count: 0,
            page_count: 0,
            obj_per_page: cmin((MappedPages8k::SIZE - MappedPages8k::METADATA_SIZE) / $size, 8 * 64),
            empty_count: 0,
            empty_slabs: Vec::with_capacity(Self::MAX_PAGE_LIST_SIZE),
            slabs: Vec::with_capacity(Self::MAX_PAGE_LIST_SIZE),
            full_slabs: Vec::with_capacity(Self::MAX_PAGE_LIST_SIZE)
        }
    };
}

impl SCAllocator {
    const _REBALANCE_COUNT: usize = 10_000;
    /// The maximum number of allocable pages the SCAllocator can hold.
    pub const MAX_PAGE_LIST_SIZE: usize = 122*10; // ~10 MiB

    /// Creates a new SCAllocator and initializes the page lists to have a capacity of `MAX_PAGE_LIST_SIZE`.
    /// After initialization, the length of the list won't exceed the capacity. 
    pub fn new(size: usize) -> SCAllocator {
        new_sc_allocator!(size)
    }

    /// Add a page to the partial list
    fn insert_partial(&mut self, new_page: MappedPages8k) {
        self.slabs.push(new_page);
        // Any recently used page, we move to the front of the list
        if self.slabs.len() > 1 {
            let mp = self.slabs.swap_remove(0);
            self.slabs.push(mp);
        }
    }

    /// Add page to empty list.
    fn insert_empty(&mut self, new_page: MappedPages8k) {
        self.empty_slabs.push(new_page);
        self.empty_count += 1;
    }

    /// Add page to full list.
    fn insert_full(&mut self, new_page: MappedPages8k) {
        self.full_slabs.push(new_page);
        // Any recently used page, we move to the front of the list
        if self.full_slabs.len() > 1 {
            let mp = self.full_slabs.swap_remove(0);
            self.full_slabs.push(mp);
        }
    }

    fn remove_empty(&mut self) -> Option<MappedPages8k> {
        self.empty_slabs.pop().map(|mp| {self.empty_count -= 1; mp} )
    }

    fn remove_partial(&mut self, id: usize) -> MappedPages8k {
        self.slabs.swap_remove(id)
    }

    fn remove_full(&mut self, id: usize) -> MappedPages8k {
        self.full_slabs.swap_remove(id)
    }

    /// Move a page from `slabs` to `empty_slabs`.
    fn move_partial_to_empty(&mut self, id: usize) {
        let page = self.remove_partial(id);
        self.insert_empty(page);
    }

    /// Move a page from `slabs` to `full_slabs`.
    fn move_partial_to_full(&mut self, id: usize) {
        let page = self.remove_partial(id);
        self.insert_full(page);
    }

    /// Move a page from `full_slabs` to `slab`.
    fn move_full_to_partial(&mut self, id: usize) {
        let page = self.remove_full(id);
        self.insert_partial(page);
    }

    /// Tries to allocate a block of memory with respect to the `layout`.
    /// Searches within already allocated slab pages, if no suitable spot is found
    /// will try to use a page from the empty page list.
    ///
    /// # Arguments
    ///  * `sc_layout`: This is not the original layout but adjusted for the
    ///     SCAllocator size (>= original).
    fn try_allocate_from_pagelist(&mut self, sc_layout: Layout) -> *mut u8 {
        // TODO: Do we really need to check multiple slab pages (due to alignment)
        // If not we can get away with a singly-linked list and have 8 more bytes
        // for the bitfield in an ObjectPage.
        let mut need_to_move = false;
        let mut ret_ptr = ptr::null_mut();
        let mut list_id = 0;

        for (id, slab_page) in self.slabs.iter_mut().enumerate() {
            let page = slab_page.as_objectpage8k_mut();
            let ptr = page.allocate(sc_layout);
            if !ptr.is_null() {
                if page.is_full() {
                    need_to_move = true;
                    list_id = id;
                    // trace!("move {:p} partial -> full", page);
                }
                self.allocation_count += 1;
                ret_ptr = ptr;
                break;
            } else {
                continue;
            }   
        }

        if need_to_move {
            self.move_partial_to_full(list_id);
        }

        // // Periodically rebalance page-lists (since dealloc can't do it for us)
        // if self.allocation_count % SCAllocator::<P>::REBALANCE_COUNT == 0 {
        //     self.check_page_assignments();
        // }

        ret_ptr
    }

    /// Refill the SCAllocator
    /// 
    /// # Warning
    /// This should only be used to insert `MAX_PAGE_LIST_SIZE` number of pages to the heap.
    /// Any more, and the heap will not be able to store them.
    pub fn refill(&mut self, mut mp: MappedPages8k, heap_id: usize) -> Result<(), &'static str> {
        if self.page_count >= Self::MAX_PAGE_LIST_SIZE {
            error!("Page limit ({} pages) of SCAllocator has been reached!", Self::MAX_PAGE_LIST_SIZE);
            return Err("Page limit of SCAllocator has been reached!");
        }
        let page = mp.as_objectpage8k_mut();
        page.bitfield_mut().initialize(self.size, MappedPages8k::SIZE - MappedPages8k::METADATA_SIZE);
        page.heap_id = heap_id;
    
        // trace!("adding page to SCAllocator {:p}", page);
        self.insert_empty(mp);
        self.page_count += 1;

        Ok(())
    }

    /// Returns an empty page from the allocator if available.
    pub fn retrieve_empty_page(&mut self) -> Option<MappedPages8k> {
        self.remove_empty().map(|mp| {self.page_count -= 1; mp} )
    }

    /// Allocates a block of memory descriped by `layout`.
    ///
    /// Returns a pointer to a valid region of memory or an
    /// Error.
    ///
    /// The function may also move around pages between lists
    /// (empty -> partial or partial -> full).
    pub fn allocate(&mut self, layout: Layout) -> Result<NonNull<u8>, &'static str> {
        // trace!(
        //     "SCAllocator({}) is trying to allocate {:?}, {}",
        //     self.size,
        //     layout, 
        //     MappedPages8k::SIZE - CACHE_LINE_SIZE
        // );
        assert!(layout.size() <= self.size);
        assert!(self.size <= (MappedPages8k::SIZE - CACHE_LINE_SIZE));
        let new_layout = unsafe { Layout::from_size_align_unchecked(self.size, layout.align()) };
        assert!(new_layout.size() >= layout.size());

        let ptr = {
            // Try to allocate from partial slabs,
            // if we fail check if we have empty pages and allocate from there
            let mut ptr = self.try_allocate_from_pagelist(new_layout);
            if ptr.is_null() {
                if let Some(mut empty_page) =  self.remove_empty() {
                    ptr = empty_page.as_objectpage8k_mut().allocate(layout);
                    debug_assert!(!ptr.is_null(), "Allocation must have succeeded here.");

                    // trace!(
                    //     "move {:p} empty -> partial",
                    //     empty_page.start_address(),
                    // );
                    // Move empty page to partial pages
                    self.insert_partial(empty_page);
                } 
                ptr

            } else {
                ptr
            }
        };

        let res = NonNull::new(ptr).ok_or("AllocationError::OutOfMemory");

        // if !ptr.is_null() {
        //     trace!(
        //         "SCAllocator({}) allocated ptr=0x{:x}",
        //         self.size,
        //         ptr as usize
        //     );
        // }

        res
    }

    /// Deallocates a previously allocated `ptr` described by `Layout`.
    ///
    /// May return an error in case an invalid `layout` is provided.
    /// The function may also move internal slab pages between lists partial -> empty
    /// or full -> partial lists.
    ///
    /// # Safety
    /// The caller must ensure that `ptr` argument is returned from [`Self::allocate()`]
    /// and `layout` argument is correct.
    pub unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) -> Result<(), &'static str> {
        assert!(layout.size() <= self.size);
        assert!(self.size <= (MappedPages8k::SIZE - CACHE_LINE_SIZE));
        // trace!(
        //     "SCAllocator({}) is trying to deallocate ptr = {:p} layout={:?} P.size= {}",
        //     self.size,
        //     ptr,
        //     layout,
        //     MappedPages8k::SIZE
        // );

        // let page_addr = (ptr.as_ptr() as usize) & !(MappedPages8k::SIZE - 1) as usize;
        let page_vaddr = VirtualAddress::new((ptr.as_ptr() as usize) & !(MappedPages8k::SIZE - 1) as usize)
            .ok_or("pointer to deallocate was an invalid virtual address")?;

        // Figure out which page we are on and retrieve a reference to it
        let new_layout = Layout::from_size_align_unchecked(self.size, layout.align());

        let (ret, slab_page_is_empty, slab_page_was_full, list_id) = {
            // find slab page from partial slabs
            let mut page = self.slabs.iter_mut().enumerate()
                .find(|(_id,mp)| mp.start_address() == page_vaddr);
            
            // if it was not in the partial slabs then it should be in the full slabs
            if page.is_none() {
                page = self.full_slabs.iter_mut().enumerate()
                .find(|(_id,mp)| mp.start_address() == page_vaddr)
            }

            let mp = page.ok_or("could not find page to deallocate from")?;

            let list_id = mp.0;
            let mapped_page = mp.1;
            let slab_page = mapped_page.as_objectpage8k_mut();

            let slab_page_was_full = slab_page.is_full();
            let ret = slab_page.deallocate(ptr, new_layout);
            debug_assert!(ret.is_ok(), "Slab page deallocate won't fail at the moment");
            (ret, slab_page.is_empty(self.obj_per_page), slab_page_was_full, list_id)
        };

        if slab_page_is_empty {
            // We need to move it from self.slabs -> self.empty_slabs
            // trace!("move {:p} partial -> empty", page_vaddr);
            self.move_partial_to_empty(list_id);
        } else if slab_page_was_full {
            // We need to move it from self.full_slabs -> self.slabs
            // trace!("move {:p} full -> partial", page_vaddr);
            self.move_full_to_partial(list_id);
        }

        ret
    }
}
