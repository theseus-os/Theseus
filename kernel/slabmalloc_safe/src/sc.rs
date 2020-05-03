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
/// It maintains three internal lists of objects that implement `AllocablePage`
/// from which it can allocate memory.
///
///  * `empty_slabs`: Is a list of pages that the SCAllocator maintains, but
///    has 0 allocations in them, these can be given back to a requestor in case
///    of reclamation.
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
///
/// If an allocation returns `OutOfMemory` a client using SCAllocator can refill
/// it using the `refill` function.
pub struct SCAllocator {
    /// Maximum possible allocation size for this `SCAllocator`.
    pub(crate) size: usize,
    /// Keeps track of succeeded allocations.
    pub(crate) allocation_count: usize,
    /// max objects per page
    pub(crate) obj_per_page: usize,
    /// Array to hold empty MappedPages (nothing allocated in these).
    pub(crate) empty_slabs: [Option<MappedPages8k>; Self::PAGE_LIST_SIZE],
    /// Array to hold partially used MappedPages (some objects allocated but pages are not full).
    pub(crate) slabs: [Option<MappedPages8k>; Self::PAGE_LIST_SIZE],
    /// Array to hold full MappedPages (everything allocated in these don't need to search them).
    pub(crate) full_slabs: [Option<MappedPages8k>; Self::PAGE_LIST_SIZE],
}

/// Creates an instance of a scallocator, we do this in a macro because we
/// re-use the code in const and non-const functions
macro_rules! new_sc_allocator {
    ($size:expr) => {
        SCAllocator {
            size: $size,
            allocation_count: 0,
            obj_per_page: cmin((MappedPages8k::SIZE - MappedPages8k::METADATA_SIZE) / $size, 8 * 64),
            empty_slabs: [None; Self::PAGE_LIST_SIZE],
            slabs: [None; Self::PAGE_LIST_SIZE],
            full_slabs: [None; Self::PAGE_LIST_SIZE],
        }
    };
}

impl SCAllocator {
    const _REBALANCE_COUNT: usize = 10_000;
    pub const PAGE_LIST_SIZE: usize = 15; //8 MiB

    /// Create a new SCAllocator.
    #[cfg(feature = "unstable")]
    pub const fn new(size: usize) -> SCAllocator {
        new_sc_allocator!(size)
    }

    #[cfg(not(feature = "unstable"))]
    pub fn new(size: usize) -> SCAllocator {
        new_sc_allocator!(size)
    }

    /// Returns the maximum supported object size of this allocator.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Add a page to the partial list
    fn insert_partial(&mut self, mut new_page: MappedPages8k) -> Result<(), &'static str> {
        // find the first index that is empty and insert the page there
        for (idx, page) in self.slabs.iter_mut().enumerate() {
            if page.is_none() {
                new_page.as_objectpage8k_mut().list_id = idx;
                *page = Some(new_page);
                return Ok(());
            }
        }

        Err("There was no available slot in the partial list")
    }

    /// Add page to empty list.
    fn insert_empty(&mut self, mut new_page: MappedPages8k) -> Result<(), &'static str> {
        // find the first index that is empty and insert the page there
        for (idx, page) in self.empty_slabs.iter_mut().enumerate() {
            if page.is_none() {
                new_page.as_objectpage8k_mut().list_id = idx;
                *page = Some(new_page);
                return Ok(());
            }
        }

        Err("There was no available slot in the empty list")
    }

    /// Add page to full list.
    fn insert_full(&mut self, mut new_page: MappedPages8k) -> Result<(), &'static str> {
        // find the first index that is empty and insert the page there
        for (idx, page) in self.full_slabs.iter_mut().enumerate() {
            if page.is_none() {
                new_page.as_objectpage8k_mut().list_id = idx;
                *page = Some(new_page);
                return Ok(());
            }
        }

        Err("There was no available slot in the full list")
    }

    fn remove_empty(&mut self) -> Option<MappedPages8k> {
        let mut mp = None;
        // find the first index that has some page and return that page
        for page in self.empty_slabs.iter_mut() {
            if page.is_some() {
                core::mem::swap(&mut mp, page);
                break;
            }
        }
        mp
    }

    fn remove_partial(&mut self, idx: usize) -> Option<MappedPages8k> {
        let mut mp = None;
        core::mem::swap(&mut self.slabs[idx], &mut mp);
        assert!(mp.is_some());
        mp
    }

    fn remove_full(&mut self, idx: usize) -> Option<MappedPages8k> {
        let mut mp = None;
        core::mem::swap(&mut self.full_slabs[idx], &mut mp);
        assert!(mp.is_some());        
        mp
    }

    /// Move a page from `slabs` to `empty_slabs`.
    fn move_partial_to_empty(&mut self, idx: usize) -> Result<(), &'static str>{
        let page = self.remove_partial(idx).ok_or("move_partial_to_empty: Could not find page in partial list!")?;
        self.insert_empty(page)
    }

    /// Move a page from `slabs` to `full_slabs`.
    fn move_partial_to_full(&mut self, idx: usize) -> Result<(), &'static str> {
        let page = self.remove_partial(idx).ok_or("move_partial_to_full: Could not find page in partial list!")?;
        self.insert_full(page)
    }

    /// Move a page from `full_slabs` to `slab`.
    fn move_full_to_partial(&mut self, idx: usize) -> Result<(), &'static str> {
        let page = self.remove_full(idx).ok_or("move_full_to_partial: Could not find page in full list!")?;
        self.insert_partial(page)
    }

    /// Tries to allocate a block of memory with respect to the `layout`.
    /// Searches within already allocated slab pages, if no suitable spot is found
    /// will try to use a page from the empty page list.
    ///
    /// # Arguments
    ///  * `sc_layout`: This is not the original layout but adjusted for the
    ///     SCAllocator size (>= original).
    fn try_allocate_from_pagelist(&mut self, sc_layout: Layout) -> Result<*mut u8, &'static str> {
        // TODO: Do we really need to check multiple slab pages (due to alignment)
        // If not we can get away with a singly-linked list and have 8 more bytes
        // for the bitfield in an ObjectPage.
        let mut need_to_move = false;
        let mut list_id = 0;
        let mut ret_ptr = ptr::null_mut();

        for slab_page in self.slabs.iter_mut() {
            match slab_page {
                Some(mp) => {
                    let page = mp.as_objectpage8k_mut();
                    let ptr = page.allocate(sc_layout);
                    if !ptr.is_null() {
                        if page.is_full() {
                            need_to_move = true;
                            list_id = page.list_id;
                            trace!("move {:p} partial -> full", page);
                        }
                        self.allocation_count += 1;
                        ret_ptr = ptr;
                    } else {
                        continue;
                    }
                }
                None => {
                    continue;
                }
            }
        };

        if need_to_move {
            // trace!("move {:p} partial -> full", );
            self.move_partial_to_full(list_id)?;
        }

        // // Periodically rebalance page-lists (since dealloc can't do it for us)
        // if self.allocation_count % SCAllocator::<P>::REBALANCE_COUNT == 0 {
        //     self.check_page_assignments();
        // }

        Ok(ret_ptr)
    }

    /// Refill the SCAllocator
    pub fn refill(&mut self, mut mp: MappedPages8k, heap_id: usize) -> Result<(), &'static str> {
        let page = mp.as_objectpage8k_mut();
        page.bitfield_mut().initialize(self.size, MappedPages8k::SIZE - MappedPages8k::METADATA_SIZE);
        page.heap_id = heap_id;
    
        // trace!("adding page to SCAllocator {:p}", page);
        self.insert_empty(mp)?;

        Ok(())
    }

    /// Returns an empty page from the allocator if available.
    /// It removes the MappedPages object from the heap pages where it is stored.
    pub fn retrieve_empty_page(&mut self) -> Option<MappedPages8k> {
        self.remove_empty()
    }

    /// Allocates a block of memory descriped by `layout`.
    ///
    /// Returns a pointer to a valid region of memory or an
    /// AllocationError.
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
            let ptr = self.try_allocate_from_pagelist(new_layout)?;
            if ptr.is_null() {
                if let Some(mut empty_page) =  self.remove_empty() {
                    let ptr = empty_page.as_objectpage8k_mut().allocate(layout);
                    debug_assert!(!ptr.is_null(), "Allocation must have succeeded here.");

                    // trace!(
                    //     "move {:p} empty -> partial",
                    //     empty_page.start_address(),
                    // );
                    // Move empty page to partial pages
                    self.insert_partial(empty_page)?;

                    ptr
                } else{
                    ptr
                }
                
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
    pub fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) -> Result<(), &'static str> {
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
        let page_vaddr = VirtualAddress::new((ptr.as_ptr() as usize) & !(MappedPages8k::SIZE - 1) as usize)?;

        // // Figure out which page we are on and construct a reference to it
        // // TODO: The linked list will have another &mut reference
        // let slab_page = unsafe { mem::transmute::<VAddr, &'a mut P>(page) };
        let new_layout = unsafe { Layout::from_size_align_unchecked(self.size, layout.align()) };

        let (ret, slab_page_is_empty, slab_page_was_full, list_id) = {
            // find slab page from partial slabs
            let mut mp = self.slabs.iter_mut()
                .find(|ref mut mp| mp.as_ref().map_or(VirtualAddress::zero(), |page| page.start_address()) == page_vaddr);
            
            if mp.is_none() {
                mp = self.full_slabs.iter_mut()
                .find(|ref mut mp| mp.as_ref().map_or(VirtualAddress::zero(), |page| page.start_address()) == page_vaddr);
            }

            // if mp.is_none() {
            //     error!("No mp: {:p}", page_vaddr);

            //     for page in self.slabs.iter_mut() {
            //         if page.is_some() {
            //             error!("{:p}", page.as_ref().unwrap().start_address());
            //         }
            //     }
            //     loop{}
            // }

            let mut mapped_page = mp.ok_or("Couldn't find page for deallocation!")?;

            let slab_page = mapped_page.as_mut().unwrap().as_objectpage8k_mut();

            let slab_page_was_full = slab_page.is_full();
            let ret = slab_page.deallocate(ptr, new_layout);
            debug_assert!(ret.is_ok(), "Slab page deallocate won't fail at the moment");
            (ret, slab_page.is_empty(self.obj_per_page), slab_page_was_full, slab_page.list_id)
        };

        if slab_page_is_empty {
            // We need to move it from self.slabs -> self.empty_slabs
            // trace!("move {:p} partial -> empty", page_vaddr);
            self.move_partial_to_empty(list_id)?;
        } else if slab_page_was_full {
            // We need to move it from self.full_slabs -> self.slabs
            // trace!("move {:p} full -> partial", page_vaddr);
            self.move_full_to_partial(list_id)?;
        }

        ret
    }
}
