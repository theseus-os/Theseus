use crate::*;
use core::sync::atomic::{AtomicU64, Ordering};

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

/// This trait is used to define a page from which objects are allocated
/// in an `SCAllocator`.
///
/// The implementor of this trait needs to provide access to the page meta-data,
/// which consists of:
/// - A bitfield (to track allocations),
pub trait AllocablePage {
    /// The total size (in bytes) of the page.
    ///
    /// # Note
    /// We also assume that the address of the page will be aligned to `SIZE`.
    const SIZE: usize;

    const METADATA_SIZE: usize;

    const HEAP_ID_OFFSET: usize;

    fn clear_metadata(&mut self);
    fn bitfield(&self) -> &[AtomicU64; 8];
    fn bitfield_mut(&mut self) -> &mut [AtomicU64; 8];

    /// Tries to find a free block within `data` that satisfies `alignment` requirement.
    fn first_fit(&self, layout: Layout) -> Option<(usize, usize)> {
        let base_addr = (self as *const Self as *const u8) as usize;
        self.bitfield().first_fit(base_addr, layout, Self::SIZE, Self::METADATA_SIZE)
    }

    /// Tries to allocate an object within this page.
    ///
    /// In case the slab is full, returns a null ptr.
    fn allocate(&mut self, layout: Layout) -> *mut u8 {
        match self.first_fit(layout) {
            Some((idx, addr)) => {
                self.bitfield().set_bit(idx);
                addr as *mut u8
            }
            None => ptr::null_mut(),
        }
    }

    /// Checks if we can still allocate more objects of a given layout within the page.
    fn is_full(&self) -> bool {
        self.bitfield().is_full()
    }

    /// Checks if the page has currently no allocations.
    fn is_empty(&self, relevant_bits: usize) -> bool {
        self.bitfield().all_free(relevant_bits)
    }

    /// Deallocates a memory object within this page.
    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) -> Result<(), &'static str> {
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

    pub(crate) heap_id: usize,
    /// the index in the list where this page is stored
    pub(crate) list_id: usize,
    /// A bit-field to track free/allocated memory within `data`.
    bitfield: [AtomicU64; 8],
}



impl AllocablePage for ObjectPage8k {
    const SIZE: usize = 8192;
    const METADATA_SIZE: usize = (2 * core::mem::size_of::<usize>())  + (8*8);
    const HEAP_ID_OFFSET: usize = Self::SIZE - ((2 * core::mem::size_of::<usize>()) + (8*8));

    /// clears the metadata section of the page
    fn clear_metadata(&mut self) {
        self.heap_id = 0;
        self.list_id = 0;
        for bf in &self.bitfield {
            bf.store(0, Ordering::SeqCst);
        }
    }

    fn bitfield(&self) -> &[AtomicU64; 8] {
        &self.bitfield
    }
    
    fn bitfield_mut(&mut self) -> &mut [AtomicU64; 8] {
        &mut self.bitfield
    }

    /// Tries to find a free block within `data` that satisfies `alignment` requirement.
    fn first_fit(&self, layout: Layout) -> Option<(usize, usize)> {
        let base_addr = (self as *const Self as *const u8) as usize;
        self.bitfield().first_fit(base_addr, layout, Self::SIZE, Self::METADATA_SIZE)
    }

    /// Tries to allocate an object within this page.
    ///
    /// In case the slab is full, returns a null ptr.
    fn allocate(&mut self, layout: Layout) -> *mut u8 {
        match self.first_fit(layout) {
            Some((idx, addr)) => {
                self.bitfield().set_bit(idx);
                addr as *mut u8
            }
            None => ptr::null_mut(),
        }
    }

    /// Checks if we can still allocate more objects of a given layout within the page.
    fn is_full(&self) -> bool {
        self.bitfield().is_full()
    }

    /// Checks if the page has currently no allocations.
    fn is_empty(&self, relevant_bits: usize) -> bool {
        self.bitfield().all_free(relevant_bits)
    }

    /// Deallocates a memory object within this page.
    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) -> Result<(), &'static str> {
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
        mp_8k.as_objectpage8k_mut().clear_metadata();
        Ok(mp_8k)
    }

    // /// Return the pages represented by the MappedPages8k as an ObjectPage8k reference
    // pub(crate) fn as_objectpage8k(&self) -> &ObjectPage8k {
    //     // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
    //     unsafe {
    //         mem::transmute(self.0.start_address())
    //     }
    // }

    /// Return the pages represented by the MappedPages8k as a mutable ObjectPage8k reference
    pub(crate) fn as_objectpage8k_mut(&mut self) -> &mut ObjectPage8k {
        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        unsafe {
            mem::transmute(self.start_address())
        }
    }

    pub fn start_address(&self) -> VirtualAddress {
        self.0.start_address()
    }
}

