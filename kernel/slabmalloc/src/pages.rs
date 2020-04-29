use crate::*;
use core::sync::atomic::{AtomicU64, Ordering};
use core::ops::{Deref, DerefMut};

/// A trait defining bitfield operations we need for tracking allocated objects within a page.
pub(crate) trait Bitfield {
    fn initialize(&mut self, for_size: usize, capacity: usize);
    fn first_fit(
        &self,
        base_addr: usize,
        layout: Layout,
        page_size: usize,
        metadata_size: usize,
    ) -> Option<(usize, usize)>;
    fn is_allocated(&self, idx: usize) -> bool;
    fn set_bit(&self, idx: usize);
    fn clear_bit(&self, idx: usize);
    fn is_full(&self) -> bool;
    fn all_free(&self, relevant_bits: usize) -> bool;
}

/// Implementation of bit operations on u64 slices.
///
/// We allow deallocations (i.e. clearing a bit in the field)
/// from any thread. That's why the bitfield is a bunch of AtomicU64.
impl Bitfield for [AtomicU64] {
    /// Initialize the bitfield
    ///
    /// # Arguments
    ///  * `for_size`: Object size we want to allocate
    ///  * `capacity`: Maximum size of the buffer the bitmap maintains.
    ///
    /// Ensures that we only have free slots for what we can allocate
    /// within the page (by marking everything else allocated).
    fn initialize(&mut self, for_size: usize, capacity: usize) {
        // Set everything to allocated
        for bitmap in self.iter_mut() {
            *bitmap = AtomicU64::new(u64::max_value());
        }

        // Mark actual slots as free
        let relevant_bits = core::cmp::min(capacity / for_size, self.len() * 64);
        for idx in 0..relevant_bits {
            self.clear_bit(idx);
        }
    }

    /// Tries to find a free block of memory that satisfies `alignment` requirement.
    ///
    /// # Notes
    /// * We pass size here to be able to calculate the resulting address within `data`.
    #[inline(always)]
    fn first_fit(
        &self,
        base_addr: usize,
        layout: Layout,
        page_size: usize,
        metadata_size: usize
    ) -> Option<(usize, usize)> {
        for (base_idx, b) in self.iter().enumerate() {
            let bitval = b.load(Ordering::Relaxed);
            if bitval == u64::max_value() {
                continue;
            } else {
                let negated = !bitval;
                let first_free = negated.trailing_zeros() as usize;
                let idx: usize = base_idx * 64 + first_free;
                let offset = idx * layout.size();

                // TODO(bad): psize needs to be passed as arg
                let offset_inside_data_area = offset <= (page_size - metadata_size - layout.size());
                if !offset_inside_data_area {
                    return None;
                }

                let addr: usize = base_addr + offset;
                let alignment_ok = addr % layout.align() == 0;
                let block_is_free = bitval & (1 << first_free) == 0;
                if alignment_ok && block_is_free {
                    return Some((idx, addr));
                }
            }
        }
        None
    }

    /// Check if the bit `idx` is set.
    #[inline(always)]
    fn is_allocated(&self, idx: usize) -> bool {
        let base_idx = idx / 64;
        let bit_idx = idx % 64;
        (self[base_idx].load(Ordering::Relaxed) & (1 << bit_idx)) > 0
    }

    /// Sets the bit number `idx` in the bit-field.
    #[inline(always)]
    fn set_bit(&self, idx: usize) {
        let base_idx = idx / 64;
        let bit_idx = idx % 64;
        self[base_idx].fetch_or(1 << bit_idx, Ordering::Relaxed);
    }

    /// Clears bit number `idx` in the bit-field.
    #[inline(always)]
    fn clear_bit(&self, idx: usize) {
        let base_idx = idx / 64;
        let bit_idx = idx % 64;
        self[base_idx].fetch_and(!(1 << bit_idx), Ordering::Relaxed);
    }

    /// Checks if we could allocate more objects of a given `alloc_size` within the
    /// `capacity` of the memory allocator.
    ///
    /// # Note
    /// The ObjectPage will make sure to mark the top-most bits as allocated
    /// for large sizes (i.e., a size 512 SCAllocator will only really need 3 bits)
    /// to track allocated objects). That's why this function can be simpler
    /// than it would need to be in practice.
    #[inline(always)]
    fn is_full(&self) -> bool {
        self.iter()
            .filter(|&x| x.load(Ordering::Relaxed) != u64::max_value())
            .count()
            == 0
    }

    /// Checks if the page has currently no allocations.
    ///
    /// This is called `all_free` rather than `is_emtpy` because
    /// we already have an is_empty fn as part of the slice.
    #[inline(always)]
    fn all_free(&self, relevant_bits: usize) -> bool {
        for (idx, bitmap) in self.iter().enumerate() {
            let checking_bit_range = (idx * 64, (idx + 1) * 64);
            if relevant_bits >= checking_bit_range.0 && relevant_bits < checking_bit_range.1 {
                // Last relevant bitmap, here we only have to check that a subset of bitmap is marked free
                // the rest will be marked full
                let bits_that_should_be_free = relevant_bits - checking_bit_range.0;
                let free_mask = (1 << bits_that_should_be_free) - 1;
                return (free_mask & bitmap.load(Ordering::Relaxed)) == 0;
            }

            if bitmap.load(Ordering::Relaxed) == 0 {
                continue;
            } else {
                return false;
            }
        }

        true
    }
}


/// Holds allocated data within 2 4-KiB pages.
///
/// Has a data-section where objects are allocated from
/// and a small amount of meta-data in the form of a bitmap
/// to track allocations at the end of the page.
///
/// # Notes
/// An object of this type will be exactly 8 KiB.
/// It is marked `repr(C)` because we rely on a well defined order of struct
/// members (e.g., dealloc does a cast to find the bitfield).
#[repr(C)]
pub struct ObjectPage8k {
    /// Holds memory objects.
    #[allow(dead_code)]
    data: [u8; ObjectPage8k::SIZE -ObjectPage8k::METADATA_SIZE],

    heap_id: usize,

    /// Next element in list (used by `PageList`).
    next_mp: Option<MappedPages8k>,

    /// A bit-field to track free/allocated memory within `data`.
    bitfield: [AtomicU64; 8],
}


impl ObjectPage8k {
    const SIZE: usize = 8192;
    const METADATA_SIZE: usize = core::mem::size_of::<Option<MappedPages8k>>() + core::mem::size_of::<usize>()  + (8*8);
    const HEAP_ID_OFFSET: usize = Self::SIZE - (core::mem::size_of::<Option<MappedPages8k>>() + core::mem::size_of::<usize>() + (8*8));

    /// clears the metadata section of the page
    fn clear_metadata(&mut self) {
        self.heap_id = 0;

        // If we simply set self.next_mp to None then this causes an error message to be printed 
        // out in MappedPages::drop() due to a difference in page tables because next_mp has never been initialized.
        // This should not lead to any actual MappedPages being forgotten since it is only called when creating
        // a new MappedPages8k object. 
        let mut mp = None;
        core::mem::swap(&mut mp, &mut self.next_mp);
        core::mem::forget(mp);

        for bf in &self.bitfield {
            bf.store(0, Ordering::SeqCst);
        }
    }

    pub(crate) fn set_heap_id(&mut self, heap_id: usize){
        self.heap_id = heap_id;
    }

    pub(crate) fn bitfield(&self) -> &[AtomicU64; 8] {
        &self.bitfield
    }
    
    pub(crate) fn bitfield_mut(&mut self) -> &mut [AtomicU64; 8] {
        &mut self.bitfield
    }

    pub(crate) fn next(&mut self) -> &mut Option<MappedPages8k> {
        &mut self.next_mp
    }

    /// Tries to find a free block within `data` that satisfies `alignment` requirement.
    pub(crate) fn first_fit(&self, layout: Layout) -> Option<(usize, usize)> {
        let base_addr = (&*self as *const Self as *const u8) as usize;
        self.bitfield().first_fit(base_addr, layout, Self::SIZE, Self::METADATA_SIZE)
    }

    /// Tries to allocate an object within this page.
    ///
    /// In case the slab is full, returns a null ptr.
    pub(crate) fn allocate(&mut self, layout: Layout) -> *mut u8 {
        match self.first_fit(layout) {
            Some((idx, addr)) => {
                self.bitfield().set_bit(idx);
                addr as *mut u8
            }
            None => ptr::null_mut(),
        }
    }

    /// Checks if we can still allocate more objects of a given layout within the page.
    pub(crate) fn is_full(&self) -> bool {
        self.bitfield().is_full()
    }

    /// Checks if the page has currently no allocations.
    pub(crate) fn is_empty(&self, relevant_bits: usize) -> bool {
        self.bitfield().all_free(relevant_bits)
    }

    /// Deallocates a memory object within this page.
    pub(crate) fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) -> Result<(), &'static str> {
        // trace!(
        //     "AllocablePage deallocating ptr = {:p} with {:?}",
        //     ptr,
        //     layout
        // );
        let page_offset = (ptr.as_ptr() as usize) & (Self::SIZE - 1);
        assert!(page_offset % layout.size() == 0);
        let idx = page_offset / layout.size();
        assert!(
            self.bitfield().is_allocated(idx),
            "{:p} not marked allocated?",
            ptr
        );

        self.bitfield().clear_bit(idx);
        Ok(())
    }
}

/// A wrapper type around MappedPages which ensures that the MappedPages
/// have a size and alignment of 8 KiB and are writable.
pub struct MappedPages8k(MappedPages);

impl MappedPages8k {
    pub const SIZE: usize = ObjectPage8k::SIZE;
    pub const BUFFER_SIZE: usize = ObjectPage8k::SIZE - ObjectPage8k::METADATA_SIZE;
    pub const METADATA_SIZE: usize = ObjectPage8k::METADATA_SIZE;
    pub const HEAP_ID_OFFSET: usize = ObjectPage8k::HEAP_ID_OFFSET;
    
    /// Creates a MappedPages8k object from MappedPages that have a size and alignment of 8 KiB and are writable.
    pub fn new(mp: MappedPages) -> Result<MappedPages8k, &'static str> {
        let vaddr = mp.start_address().value();
        
        // check that the mapped pages are aligned to 8k
        if vaddr % Self::SIZE != 0 {
            error!("Trying to create a MappedPages8k but MappedPages were not aligned at 8k bytes");
            return Err("Trying to create a MappedPages8k but MappedPages were not aligned at 8k bytes");
        }

        // check that the mapped pages is writable
        if !mp.flags().is_writable() {
            error!("Trying to create a MappedPages8k but MappedPages were not writable (flags: {:?})",  mp.flags());
            return Err("Trying to create a MappedPages8k but MappedPages were not writable");
        }
        
        // check that the mapped pages size is equal in size to the page
        if Self::SIZE != mp.size_in_bytes() {
            error!("Trying to create a MappedPages8k but MappedPages were not 8 KiB (size: {} bytes)", mp.size_in_bytes());
            return Err("Trying to create a MappedPages8k but MappedPages were not 8 KiB");
        }

        let mut mp_8k = MappedPages8k(mp);
        mp_8k.clear_metadata();
        Ok(mp_8k)
    }

    /// Return the pages represented by the MappedPages8k as an ObjectPage8k reference
    fn as_objectpage8k(&self) -> &ObjectPage8k {
        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        unsafe {
            mem::transmute(self.0.start_address())
        }
    }

    /// Return the pages represented by the MappedPages8k as a mutable ObjectPage8k reference
    fn as_objectpage8k_mut(&mut self) -> &mut ObjectPage8k {
        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        unsafe {
            mem::transmute(self.0.start_address())
        }
    }

    pub fn start_address(&self) -> VirtualAddress {
        self.0.start_address()
    }
}

/// Deref of MappedPages8k will return a reference to the pages it represents
/// cast as an ObjectPage8k
impl Deref for MappedPages8k {
    type Target = ObjectPage8k;
    fn deref(&self) -> &ObjectPage8k {
        self.as_objectpage8k()
    }
}

/// DerefMut of MappedPages8k will return a mutable reference to the pages it represents
/// cast as an ObjectPage8k
impl DerefMut for MappedPages8k {
    fn deref_mut(&mut self) -> &mut ObjectPage8k {
        self.as_objectpage8k_mut()
    }
}


/// A recursive data structure representing a list of MappedPages8k.
/// Rather than using pointers to create a linked list, we store each successive 
/// MappedPages8k object within the pages represented by the previous MappedPages8k.
pub(crate) struct PageList {
    /// The head of the list.
    pub(crate) head: Option<MappedPages8k>,
    /// Number of elements in the list.
    pub(crate) elements: usize,
}

impl PageList {
    #[cfg(feature = "unstable")]
    pub(crate) const fn new() -> PageList {
        PageList {
            head: None,
            elements: 0,
        }
    }

    #[cfg(not(feature = "unstable"))]
    pub(crate) fn new() -> PageList {
        PageList {
            head: None,
            elements: 0,
        }
    }

    pub(crate) fn iter_mut<'a>(&mut self) -> MappedPages8kIterMut<'a> {
        let m = match self.head {
            None => Rawlink::none(),
            Some(ref mut m) => Rawlink::some(m),
        };

        MappedPages8kIterMut {
            head: m,
            phantom: core::marker::PhantomData,
        }
    }

    /// Inserts `new_head` at the front of the list.
    pub(crate) fn insert_front(&mut self, mut new_head: MappedPages8k) {
        match self.head {
            None => {
                self.head = Some(new_head);
            }
            Some(ref mut head) => {
                mem::swap(head, &mut new_head);
                *head.next() = Some(new_head);
            }
        }

        self.elements += 1;
    }

    /// Removes from the list and returns the MappedPages8k with starting address `page_start_addr`.
    pub(crate) fn remove_from_list(&mut self, page_start_addr: VirtualAddress) -> Option<MappedPages8k> {
        let mut head = &mut self.head;
        match head {
            None => return None,
            Some(ref mut mp) => {
                if mp.start_address() == page_start_addr {
                    let mut found_mp = self.head.take();
                    self.head = found_mp.as_mut().unwrap().next().take();
                    self.elements -= 1;
                    return found_mp;
                }
            }
        };

        loop {
            match head {
                None => return None,
                Some(ref mut mp) => {
                    match mp.next() {
                        None => return None,
                        Some(ref mut mp_next) => {
                            if mp_next.start_address() == page_start_addr {
                                let mut found_mp = mp.next().take();
                                let new_next = found_mp.as_mut().unwrap().next().take();
                                *mp.next() = new_next;

                                self.elements -= 1;
                                return found_mp
                            }
                            else {
                                head = mp.next();
                            }
                        }
                    }
                }
            }
        }
    }

    /// Removes and returns the element at the front of the list.
    pub(crate) fn pop(&mut self) -> Option<MappedPages8k> {
        match self.head {
            None => None,
            Some(ref mut head) => {
                let head_next = head.next().take();
                let old_head = self.head.take();
                self.head = head_next; 
                self.elements -= 1;
                old_head
            }
        }
    }

    /// Does the list contain a MappedPages8k starting with the address `addr`?
    pub(crate) fn contains(&mut self, addr: VirtualAddress) -> bool {
        for slab_page in self.iter_mut() {
            if slab_page.start_address() == addr {
                return true;
            }
        }

        false
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.elements == 0
    }

    /// Print the starting address of all the elements in the list
    pub(crate) fn print(&mut self) {
        trace!("*** page list ***");
        let page_iter = self.iter_mut();
        for page in page_iter {
            trace!("{:#X}", page.start_address())
        }
    }
}

impl Drop for PageList {
    /// Iterate through all the elements in the list and drop them
    fn drop(&mut self) {
        while !self.is_empty() {
            self.pop();
        }
    }
}

/// Simple test to check the correctness of PageList implementation.
/// Remove alignment requirement from MappedPages8k before running
pub fn test_page_list() -> Result<(), &'static str>{
    let mut list = PageList::new();
    let mut addr: [VirtualAddress; 10] = [VirtualAddress::new_canonical(0); 10];

    for i in 0..10 {
        let mp = create_mapping(8192, EntryFlags::WRITABLE)?;
        let mp_8k = MappedPages8k::new(mp)?;
        addr[i] = mp_8k.start_address();

        list.insert_front(mp_8k);
        list.print();
    }

    list.remove_from_list(addr[3]).ok_or("Couldn't find address in list")?;
    list.print();

    list.remove_from_list(addr[8]).ok_or("Couldn't find address in list")?;
    list.print();

    while !list.is_empty() {
        list.pop();
        list.print();
    }

    Ok(())


}

/// Iterate over all the pages inside a slab allocator
pub(crate) struct MappedPages8kIterMut<'a> {
    head: Rawlink<MappedPages8k>,
    phantom: core::marker::PhantomData<&'a MappedPages8k>,
}

impl<'a> Iterator for MappedPages8kIterMut<'a> {
    type Item = &'a mut MappedPages8k;

    #[inline]
    fn next(&mut self) -> Option<&'a mut MappedPages8k> {
        unsafe {
            self.head.resolve_mut().map(|next| {
                self.head = match next.next() {
                    None => Rawlink::none(),
                    Some(ref mut sp) => Rawlink::some(sp),
                };
                next
            })
        }
    }
}

/// Rawlink is a type like Option<T> but for holding a raw pointer.
///
/// We use it to link Pages together. You probably won't need
/// to use this type if you're not implementing a custom page-size.
pub struct Rawlink<T> {
    p: *mut T,
}

impl<T> Default for Rawlink<T> {
    fn default() -> Self {
        Rawlink { p: ptr::null_mut() }
    }
}

impl<T> Rawlink<T> {
    /// Like Option::None for Rawlink
    pub(crate) fn none() -> Rawlink<T> {
        Rawlink { p: ptr::null_mut() }
    }

    /// Like Option::Some for Rawlink
    pub(crate) fn some(n: &mut T) -> Rawlink<T> {
        Rawlink { p: n }
    }

    /// Convert the `Rawlink` into an Option value
    ///
    /// **unsafe** because:
    ///
    /// - Dereference of raw pointer.
    /// - Returns reference of arbitrary lifetime.
    #[allow(dead_code)]
    pub(crate) unsafe fn resolve<'a>(&self) -> Option<&'a T> {
        self.p.as_ref()
    }

    /// Convert the `Rawlink` into an Option value
    ///
    /// **unsafe** because:
    ///
    /// - Dereference of raw pointer.
    /// - Returns reference of arbitrary lifetime.
    pub(crate) unsafe fn resolve_mut<'a>(&mut self) -> Option<&'a mut T> {
        self.p.as_mut()
    }

    /// Return the `Rawlink` and replace with `Rawlink::none()`
    #[allow(dead_code)]
    pub(crate) fn take(&mut self) -> Rawlink<T> {
        mem::replace(self, Rawlink::none())
    }
}

