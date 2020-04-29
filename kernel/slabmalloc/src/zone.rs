//! A ZoneAllocator to allocate arbitrary object sizes (up to `ZoneAllocator::MAX_ALLOC_SIZE`)
//!
//! The ZoneAllocator achieves this by having many `SCAllocator`

use crate::*;

/// Creates an instance of a zone, we do this in a macro because we
/// re-use the code in const and non-const functions
///
/// We can get rid of this once the const fn feature is fully stabilized.
macro_rules! new_zone {
    ($x:expr) => {
        ZoneAllocator {
            heap_id: $x,
            allocated: 0,
            used: 0,
            // TODO(perf): We should probably pick better classes
            // rather than powers-of-two (see SuperMalloc etc.)
            small_slabs: [
                SCAllocator::new(1 << 3),  // 8
                SCAllocator::new(1 << 4),  // 16
                SCAllocator::new(1 << 5),  // 32
                SCAllocator::new(1 << 6),  // 64
                SCAllocator::new(1 << 7),  // 128
                SCAllocator::new(1 << 8),  // 256
                SCAllocator::new(1 << 9),  // 512
                SCAllocator::new(1 << 10), // 1024 (TODO: maybe get rid of this class?)
                SCAllocator::new(1 << 11), // 2048 (TODO: maybe get rid of this class?)
                SCAllocator::new(1 << 12), // 4096 
                SCAllocator::new(ZoneAllocator::MAX_ALLOC_SIZE),    // (can't do 8192 because of metadata in ObjectPage)
            ]
        }
    };
}

/// A zone allocator for arbitrary sized allocations.
///
/// Has a bunch of `SCAllocator` and through that can serve allocation
/// requests for many different object sizes up to (MAX_SIZE_CLASSES) by selecting
/// the right `SCAllocator` for allocation and deallocation.
///
/// The allocator provides the refill function `refill`
/// to provide the underlying `SCAllocator` with more memory in case it runs out.
pub struct ZoneAllocator {
    pub heap_id: usize,
    /// The total number of bytes given to this allocator from the system.
    allocated: usize,
    /// The total number of bytes actually allocated (in use).
    used: usize,
    small_slabs: [SCAllocator; ZoneAllocator::MAX_BASE_SIZE_CLASSES],
}

impl<'a> Default for ZoneAllocator {
    fn default() -> ZoneAllocator {
        new_zone!(0)
    }
}

#[allow(dead_code)]
enum Slab {
    Base(usize),
    Large(usize),
    Unsupported,
}


impl ZoneAllocator {
    /// Maximum size that's allocated within 2 pages as given by MappedPages8k.
    /// This is also the maximum object size that this allocator can handle.
    pub const MAX_ALLOC_SIZE: usize = MappedPages8k::SIZE - MappedPages8k::METADATA_SIZE;

    /// Maximum size which is allocated with MappedPages8k.
    ///
    /// e.g. this is 8 KiB - x bytes of meta-data.
    pub const MAX_BASE_ALLOC_SIZE: usize = ZoneAllocator::MAX_ALLOC_SIZE;

    /// How many allocators of type SCAllocator<MappedPages8k> we have.
    pub const MAX_BASE_SIZE_CLASSES: usize = 11;

    /// The set of sizes the allocator has lists for.
    pub const BASE_ALLOC_SIZES: [usize; ZoneAllocator::MAX_BASE_SIZE_CLASSES] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, ZoneAllocator::MAX_BASE_ALLOC_SIZE];

    /// A slab must have greater than this number of empty pages to return one.
    const SLAB_EMPTY_PAGES_THRESHOLD: usize = 0;

    #[cfg(feature = "unstable")]
    pub const fn new(heap_id: usize) -> ZoneAllocator {
        new_zone!(heap_id)
    }

    #[cfg(not(feature = "unstable"))]
    pub fn new(heap_id: usize) -> ZoneAllocator {
        new_zone!(heap_id)
    }


    /// Return maximum size an object of size `current_size` can use.
    ///
    /// Used to optimize `realloc`.
    pub fn get_max_size(current_size: usize) -> Option<usize> {
        match current_size {
            0..=8 => Some(8),
            9..=16 => Some(16),
            17..=32 => Some(32),
            33..=64 => Some(64),
            65..=128 => Some(128),
            129..=256 => Some(256),
            257..=512 => Some(512),
            513..=1024 => Some(1024),
            1025..=2048 => Some(2048),
            2049..=4096 => Some(4096),
            4097..=ZoneAllocator::MAX_ALLOC_SIZE => Some(ZoneAllocator::MAX_ALLOC_SIZE),
            _ => None,
        }
    }

    /// Figure out index into zone array to get the correct slab allocator for that size.
    fn get_slab(requested_size: usize) -> Slab {
        match requested_size {
            0..=8 => Slab::Base(0),
            9..=16 => Slab::Base(1),
            17..=32 => Slab::Base(2),
            33..=64 => Slab::Base(3),
            65..=128 => Slab::Base(4),
            129..=256 => Slab::Base(5),
            257..=512 => Slab::Base(6),
            513..=1024 => Slab::Base(7),
            1025..=2048 => Slab::Base(8),
            2049..=4096 => Slab::Base(9),
            4097..=ZoneAllocator::MAX_ALLOC_SIZE => Slab::Base(10),
            _ => Slab::Unsupported,
        }
    }
}

impl<'a> ZoneAllocator {

    /// Returns a MappedPages8k from the SCAllocator with the maximum number of empty pages,
    /// if there are more empty pages than the threshold, and updates the heap statistics.
    pub fn retrieve_empty_page(
        &mut self,
        heap_empty_page_threshold: usize
    ) -> Option<MappedPages8k> {
        if self.empty_pages() <= heap_empty_page_threshold {
            return None;
        }
        else {
            for slab in self.small_slabs.iter_mut() {
                let empty_pages = slab.empty_slabs.elements;
                if empty_pages > ZoneAllocator::SLAB_EMPTY_PAGES_THRESHOLD {
                    self.allocated -= MappedPages8k::BUFFER_SIZE;
                    return slab.retrieve_empty_page()
                }
            }
        }
        None
    }

    /// Moves empty pages between the SCAllocators
    pub fn exchange_pages_within_heap(&mut self, layout: Layout) -> Result<(), &'static str> {
        let mp = self.retrieve_empty_page(0).ok_or("Couldn't find an empty page to exchange within the heap")?;
        self.refill(layout, mp)
    }  

    /// The total number of empty pages in this zone allocator
    pub fn empty_pages(&self) -> usize {
        let mut empty_pages = 0;
        for sca in &self.small_slabs {
            empty_pages += sca.empty_slabs.elements;
        }
        empty_pages
    }
}

unsafe impl<'a> crate::Allocator<'a> for ZoneAllocator {
    /// Allocate a pointer to a block of memory described by `layout`
    /// and update heap statistics.
    fn allocate(&mut self, layout: Layout) -> Result<NonNull<u8>, &'static str> {
        match ZoneAllocator::get_slab(layout.size()) {
            Slab::Base(idx) => {
                match self.small_slabs[idx].allocate(layout) {
                    Ok(ptr) => {
                        self.used += layout.size();
                        Ok(ptr)
                    },
                    Err(_e) => {
                        self.exchange_pages_within_heap(layout)?;
                        self.small_slabs[idx].allocate(layout).and_then(|ptr| {self.used += layout.size(); Ok(ptr)} )
                    }
                }
            }
            Slab::Large(_idx) => Err("AllocationError::InvalidLayout"),
            Slab::Unsupported => Err("AllocationError::InvalidLayout"),
        }
    }

    /// Deallocates a pointer to a block of memory, which was
    /// previously allocated by `allocate`, and updates heap statistics.
    ///
    /// # Arguments
    ///  * `ptr` - Address of the memory location to free.
    ///  * `layout` - Memory layout of the block pointed to by `ptr`.
    fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) -> Result<(), &'static str> {
        match ZoneAllocator::get_slab(layout.size()) {
            Slab::Base(idx) => {
                self.small_slabs[idx].deallocate(ptr, layout).and_then(|()| {self.used -= layout.size(); Ok(())})},
            Slab::Large(_idx) => Err("AllocationError::InvalidLayout"),
            Slab::Unsupported => Err("AllocationError::InvalidLayout"),
        }
    }

    /// Refills the SCAllocator for a given Layout with a MappedPages8k object
    /// and updates heap statistics.
    fn refill(
        &mut self,
        layout: Layout,
        mp: MappedPages8k,
    ) -> Result<(), &'static str> {
        match ZoneAllocator::get_slab(layout.size()) {
            Slab::Base(idx) => {
                self.small_slabs[idx].refill(mp, self.heap_id).and_then(|()| {self.allocated += MappedPages8k::BUFFER_SIZE; Ok(())})
            }
            Slab::Large(_idx) => Err("AllocationError::InvalidLayout"),
            Slab::Unsupported => Err("AllocationError::InvalidLayout"),
        }
    }
}

