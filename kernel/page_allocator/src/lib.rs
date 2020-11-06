//! Provides an allocator for virtual memory pages.
//! The minimum unit of allocation is a single page. 
//! 
//! This also supports early allocation of pages (up to 32 individual chunks)
//! before heap allocation is available, and does so behind the scenes using the same single interface. 
//! 
//! Once heap allocation is available, it uses a dynamically-allocated list of page chunks to track allocations.
//! 
//! The core allocation function is [`allocate_pages_deferred()`](fn.allocate_pages_deferred.html), 
//! but there are several convenience functions that offer simpler interfaces for general usage. 

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate kernel_config;
extern crate memory_structs;
extern crate spin;

use alloc::collections::LinkedList;
use core::{
	fmt,
	ops::Deref,
};
use kernel_config::memory::*;
use memory_structs::{VirtualAddress, Page, PageRange};
use spin::Mutex;


/// The single, system-wide list of free virtual memory pages.
/// Currently this list includes both free and allocated chunks of pages together in the same list,
/// but it may be better to separate them in the future,
/// especially when we transition to a RB-tree or a better data structure to track allocated pages. 
static FREE_PAGE_LIST: Mutex<StaticArrayLinkedList<Chunk>> = Mutex::new(StaticArrayLinkedList::Array([
	// The list of available pages starts with the kernel text region (we should rename this).
	Some(Chunk { 
		allocated: false,
		pages: PageRange::new(
			Page::containing_address(VirtualAddress::new_canonical(KERNEL_TEXT_START)),
			Page::containing_address(VirtualAddress::new_canonical(MAX_VIRTUAL_ADDRESS)),
			// Page::containing_address(VirtualAddress::new_canonical(KERNEL_TEXT_START + KERNEL_TEXT_MAX_SIZE - BYTES_PER_ADDR)), // inclusive range
		)
	}),
	// It also includes the kernel stack and heap regions. 
	Some(Chunk { 
		allocated: false,
		pages: PageRange::new(
			Page::containing_address(VirtualAddress::new_canonical(KERNEL_STACK_ALLOCATOR_BOTTOM)),
			Page::containing_address(VirtualAddress::new_canonical(KERNEL_HEAP_START + KERNEL_HEAP_MAX_SIZE - BYTES_PER_ADDR)), // inclusive range
		)
	}),
	// It also includes the lower parts of the address space needed for booting up other CPU cores (APs).
	// See the `multicore_bringup` crate. 
	Some(Chunk { 
		allocated: false,
		pages: PageRange::new(
			Page::containing_address(VirtualAddress::new_canonical(0xF000)),
			Page::containing_address(VirtualAddress::new_canonical(0x1_0000)), // inclusive range
		)
	}),
	// In the future, we can add additional items here, e.g., the entire virtual address space.
	// NOTE: we must never include the range of addresses covered by the 510th entry in the top-level (P4) page table,
	// since that is used for the recursive page table mapping.
	// Those forbidden addresses include the range from `0xFFFF_FF00_0000_0000` to `0xFFFF_FF80_0000_0000`.
	None, None, None, None, None, 
	None, None, None, None, None, None, None, None,
	None, None, None, None, None, None, None, None,
	None, None, None, None, None, None, None, None,
]));


/// A range of contiguous pages and whether they're allocated or free.
#[derive(Debug, Clone)]
struct Chunk {
	/// Whether or not this Chunk is currently allocated. If false, it is free.
	allocated: bool,
	/// The Pages covered by this chunk, an inclusive range. 
	pages: PageRange,
}
impl Chunk {
	fn as_allocated_pages(&self) -> AllocatedPages {
		AllocatedPages {
			pages: self.pages.clone(),
		}
	}

	/// Returns a new `Chunk` with an empty range of pages. 
	fn empty() -> Chunk {
		Chunk {
			allocated: false,
			pages: PageRange::empty(),
		}
	}
}
impl Deref for Chunk {
    type Target = PageRange;
    fn deref(&self) -> &PageRange {
        &self.pages
    }
}



/// Represents a range of allocated `VirtualAddress`es, specified in `Page`s. 
/// 
/// These pages are not initially mapped to any physical memory frames, you must do that separately
/// in order to actually use their memory; see the `MappedPages` type for more. 
/// 
/// This object represents ownership of the allocated virtual pages;
/// if this object falls out of scope, its allocated pages will be auto-deallocated upon drop. 
/// 
/// TODO: implement proper deallocation for `AllocatedPages` upon drop.
pub struct AllocatedPages {
	pages: PageRange,
}
impl Deref for AllocatedPages {
    type Target = PageRange;
    fn deref(&self) -> &PageRange {
        &self.pages
    }
}
impl fmt::Debug for AllocatedPages {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("AllocatedPages")
            .field("", &self.pages)
            .finish()
	}
}

impl AllocatedPages {
	/// Returns an empty AllocatedPages object that performs no page allocation. 
    /// Can be used as a placeholder, but will not permit any real usage. 
    pub const fn empty() -> AllocatedPages {
        AllocatedPages {
			pages: PageRange::empty()
		}
	}

	/// Merges the given `AllocatedPages` object `ap` into this `AllocatedPages` object (`self`).
	/// This is just for convenience and usability purposes, it performs no allocation or remapping.
    ///
	/// The `ap` must be virtually contiguous and come immediately after `self`,
	/// that is, `self.end` must equal `ap.start`. 
	/// If this condition is met, `self` is modified and `Ok(())` is returned,
	/// otherwise `Err(ap)` is returned.
	pub fn merge(&mut self, ap: AllocatedPages) -> Result<(), AllocatedPages> {
		// make sure the pages are contiguous
		if *ap.start() != (*self.end() + 1) {
			return Err(ap);
		}
		self.pages = PageRange::new(*self.start(), *ap.end());
		// ensure the now-merged AllocatedPages doesn't run its drop handler and free its pages.
		core::mem::forget(ap); 
		Ok(())
	}

	/// Splits this `AllocatedPages` into two separate `AllocatedPages` objects:
	/// * `[beginning : at_page - 1]`
	/// * `[at_page : end]`
	/// 
	/// Depending on the size of this `AllocatedPages`, either one of the 
	/// returned `AllocatedPages` objects may be empty. 
	/// 
	/// Returns `None` if `at_page` is not within the bounds of this `AllocatedPages`.
	pub fn split(self, at_page: Page) -> Option<(AllocatedPages, AllocatedPages)> {
		let end_of_first = at_page - 1;
		if at_page > *self.pages.start() && end_of_first <= *self.pages.end() {
			let first  = PageRange::new(*self.pages.start(), at_page - 1);
			let second = PageRange::new(at_page, *self.pages.end());
			Some((
				AllocatedPages { pages: first }, 
				AllocatedPages { pages: second },
			))
		} else {
			None
		}
	}
}

// impl Drop for AllocatedPages {
//     fn drop(&mut self) {
// 		trace!("page_allocator: deallocate_pages is not yet implemented, trying to dealloc: {:?}", _pages);
// 		unimplemented!();
// 		Ok(())
//     }
// }


/// A convenience wrapper that abstracts either a `LinkedList<T>` or a primitive array `[T; N]`.
/// 
/// This allows the caller to create an array statically in a const context, 
/// and then abstract over both that and the inner `LinkedList` when using it. 
/// 
/// TODO: use const generics to allow this to be of any arbitrary size beyond 32 elements.
pub enum StaticArrayLinkedList<T> {
	Array([Option<T>; 32]),
	LinkedList(LinkedList<T>),
}
impl<T> StaticArrayLinkedList<T> {
	/// Push the given `value` onto the end of this collection.
	pub fn push_back(&mut self, value: T) -> Result<(), T> {
		match self {
			StaticArrayLinkedList::Array(arr) => {
				for elem in arr {
					if elem.is_none() {
						*elem = Some(value);
						return Ok(());
					}
				}
				error!("Out of space in array, failed to insert value.");
				Err(value)
			}
			StaticArrayLinkedList::LinkedList(ll) => {
				ll.push_back(value);
				Ok(())
			}
		}
	}

	/// Push the given `value` onto the front end of this collection.
	/// If the inner collection is an array, then this is an expensive operation
	/// with linear time complexity (on the size of the array) 
	/// because it requires all successive elements to be right-shifted. 
	pub fn push_front(&mut self, value: T) -> Result<(), T> {
		match self {
			StaticArrayLinkedList::Array(arr) => {
				// The array must have space for at least one element at the end.
				if let Some(None) = arr.last() {
					arr.rotate_right(1);
					arr[0].replace(value);
					Ok(())
				} else {
					error!("Out of space in array, failed to insert value.");
					Err(value)
				}
			}
			StaticArrayLinkedList::LinkedList(ll) => {
				ll.push_front(value);
				Ok(())
			}
		}
	}

	/// Converts the contained collection from a primitive array into a LinkedList.
	/// If the contained collection is already using heap allocation, this is a no-op.
	/// 
	/// Call this function once heap allocation is available. 
	pub fn convert_to_heap_allocated(&mut self) {
		let new_ll = match self {
			StaticArrayLinkedList::Array(arr) => {
				let mut ll = LinkedList::<T>::new();
				for elem in arr {
					if let Some(e) = elem.take() {
						ll.push_back(e);
					}
				}
				ll
			}
			StaticArrayLinkedList::LinkedList(_ll) => return,
		};
		*self = StaticArrayLinkedList::LinkedList(new_ll);
	}

	/// Returns a forward iterator over references to items in this collection.
	pub fn iter(&self) -> impl Iterator<Item = &T> {
		let mut iter_a = None;
		let mut iter_b = None;
		match self {
			StaticArrayLinkedList::Array(arr)     => iter_a = Some(arr.iter().flatten()),
			StaticArrayLinkedList::LinkedList(ll) => iter_b = Some(ll.iter()),
		}
		iter_a.into_iter().flatten().chain(iter_b.into_iter().flatten())
	}

	/// Returns a forward iterator over mutable references to items in this collection.
	pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
		let mut iter_a = None;
		let mut iter_b = None;
		match self {
			StaticArrayLinkedList::Array(arr)     => iter_a = Some(arr.iter_mut().flatten()),
			StaticArrayLinkedList::LinkedList(ll) => iter_b = Some(ll.iter_mut()),
		}
		iter_a.into_iter().flatten().chain(iter_b.into_iter().flatten())
	}
}


/// A series of pending actions related to page allocator bookkeeping,
/// which may result in heap allocation. 
/// 
/// The actions are triggered upon dropping this struct. 
/// This struct can be returned from the `allocate_pages()` family of functions 
/// in order to allow the caller to precisely control when those actions 
/// that may result in heap allocation should occur. 
/// Such actions include adding free or allocated chunks to the list of free pages or pages in use. 
/// 
/// If you don't care about precise control, simply drop this struct at any time, 
/// or ignore it with a `let _ = ...` binding to instantly drop it. 
pub struct DeferredAllocAction<'list> {
	/// A reference to the list into which we will insert the free `Chunk`s.
	free_list: &'list Mutex<StaticArrayLinkedList<Chunk>>,
	/// A reference to the list into which we will insert the allocated `Chunk`s.
	allocated_list: &'list Mutex<StaticArrayLinkedList<Chunk>>,
	/// The chunk that was marked as allocated during the page allocation. 
	/// NOTE: we don't actually need to keep track of the list of allocated chunks, 
	/// but it's handy for debugging purposes.
	allocated: Chunk,
	/// A free chunk that needs to be added back to the free list.
	free1: Chunk,
	/// Another free chunk that needs to be added back to the free list.
	free2: Chunk,
}
impl<'list> DeferredAllocAction<'list> {
	fn new<A, F1, F2>(allocated: A, free1: F1, free2: F2) -> DeferredAllocAction<'list> 
		where A:  Into<Option<Chunk>>,
			  F1: Into<Option<Chunk>>,
			  F2: Into<Option<Chunk>>,
	{
		let free_list = &FREE_PAGE_LIST;
		let allocated_list = &FREE_PAGE_LIST;
		let allocated = allocated.into().unwrap_or(Chunk::empty());
		let free1 = free1.into().unwrap_or(Chunk::empty());
		let free2 = free2.into().unwrap_or(Chunk::empty());
		DeferredAllocAction { free_list, allocated_list, allocated, free1, free2 }
	}
}
impl<'list> Drop for DeferredAllocAction<'list> {
	fn drop(&mut self) {
		// Insert the free chunks into the list, putting the larger one before the smaller one
		// and ignoring either one if it is zero-sized. 
		let size1 = self.free1.size_in_pages();
		let size2 = self.free2.size_in_pages();
		let (first, second) = if size1 > size2 {
			(&self.free1, &self.free2)
		} else {
			(&self.free2, &self.free1)
		};

		if size1 > 0 || size2 > 0 {
			let mut ll = self.free_list.lock();
			if size1 > 0 {
				ll.push_front(first.clone()).unwrap();
			}
			if size2 > 0 {
				ll.push_front(second.clone()).unwrap();
			}
		}
		if self.allocated.size_in_pages() > 0 {
			self.allocated_list.lock().push_back(self.allocated.clone()).unwrap();
		}
	}
}


/// The core page allocation routine that allocates the given number of virtual pages,
/// optionally at the requested starting `VirtualAddress`.
/// 
/// This simply reserves a range of virtual addresses, it does not allocate 
/// actual physical memory frames nor do any memory mapping. 
/// Thus, the returned `AllocatedPages` aren't directly usable until they are mapped to physical frames. 
/// 
/// Allocation is quick, technically `O(n)` but generally will allocate immediately
/// because the largest free chunks are stored at the front of the list.
/// Fragmentation isn't cleaned up until we're out of address space, but that's not really a big deal.
/// 
/// # Arguments
/// * `requested_vaddr`: if `Some`, the returned `AllocatedPages` will start at the `Page`
///   containing this `VirtualAddress`. 
///   If `None`, the first available `Page` range will be used, starting at any random virtual address.
/// * `num_pages`: the number of `Page`s to be allocated. 
/// 
/// # Return
/// If successful, returns a tuple of two items:
/// * the pages that were allocated, and
/// * an opaque struct representing details of bookkeeping-related actions that may cause heap allocation. 
///   Those actions are deferred until this returned `DeferredAllocAction` struct object is dropped, 
///   allowing the caller (such as the heap implementation itself) to control when heap allocation may occur.
pub fn allocate_pages_deferred(
	requested_vaddr: Option<VirtualAddress>,
	num_pages: usize,
) -> Result<(AllocatedPages, DeferredAllocAction<'static>), &'static str> {
	if num_pages == 0 {
		warn!("PageAllocator: requested an allocation of 0 pages... stupid!");
		return Err("cannot allocate zero pages");
	}

	let desired_start_page = requested_vaddr.map(|vaddr| Page::containing_address(vaddr));

	let mut locked_list = FREE_PAGE_LIST.lock();
	for c in locked_list.iter_mut() {
		// Look for the chunk that contains the desired address, 
		// or any chunk that is large enough (if no desired address was requested).
		let potential_start_page = desired_start_page.unwrap_or(*c.pages.start());
		let potential_end_page   = potential_start_page + num_pages - 1; // inclusive bound
		if potential_start_page >= *c.pages.start() && potential_end_page <= *c.pages.end() {
			if c.allocated {
				return Err("address already allocated");
			}
			// We've found a suitable chunk, so fall through to the rest of the loop.
		} else {
			continue;
		}
		
		// If this chunk is exactly the right size, just update it in-place as 'allocated' and return that chunk.
		if num_pages == c.pages.size_in_pages() {
			c.allocated = true;
			return Ok((
				c.as_allocated_pages(),
				DeferredAllocAction::new(None, None, None),
			));
		}

		// The new allocated chunk might start in the middle of an existing chunk,
		// so we need to break up that existing chunk into 3 possible chunks: before, newly-allocated, and after.
		let new_allocation = PageRange::new(potential_start_page, potential_end_page);
		let before = PageRange::new(*c.pages.start(), *new_allocation.start() - 1);
		let after = PageRange::new(*new_allocation.end() + 1, *c.pages.end());

		// Adjust the current chunk in place here, such that it now holds the smaller of the two free chunks;
		// the larger free chunk will be inserted into the front of the list later -- see the drop handler
		// of the `DeferredAllocAction` struct for more details.
		// However, if either chunk is zero-sized, we use the other one here.
		// At this point, both cannot be zero-sized due to the exact-sized chunk condition above.
		let extra_free_pages: PageRange; 
		if before.size_in_pages() > 0 && 
			(after.size_in_pages() == 0 || before.size_in_pages() < after.size_in_pages())
		{
			c.pages = before;
			extra_free_pages = after;
		} else {
			c.pages = after;
			extra_free_pages = before;
		}

		let allocated_chunk = Chunk {
			allocated: true,
			pages: new_allocation,
		};
		let extra_free_chunk = Chunk {
			allocated: false,
			pages: extra_free_pages,
		};
		return Ok((
			allocated_chunk.as_allocated_pages(),
			DeferredAllocAction::new(allocated_chunk, extra_free_chunk, None),
		));
	}

	error!("PageAllocator: out of virtual address space, or requested address {:#X?} ({} pages) was not covered by page allocator.",
		requested_vaddr, num_pages
	);
	Err("out of virtual address space, or requested virtual address not covered by page allocator.")
}


/// Similar to [`allocated_pages_deferred()`](fn.allocate_pages_deferred.html),
/// but accepts a size value for the allocated pages in number of bytes instead of number of pages. 
/// 
/// This function still allocates whole pages by rounding up the number of bytes. 
pub fn allocate_pages_by_bytes_deferred(
	requested_vaddr: Option<VirtualAddress>,
	num_bytes: usize,
) -> Result<(AllocatedPages, DeferredAllocAction<'static>), &'static str> {
	let num_pages = (num_bytes + PAGE_SIZE - 1) / PAGE_SIZE; // round up
	allocate_pages_deferred(requested_vaddr, num_pages)
}


/// Allocates the given number of pages with no constraints on the starting virtual address.
/// 
/// See [`allocate_pages_deferred()`](fn.allocate_pages_deferred.html) for more details. 
pub fn allocate_pages(num_pages: usize) -> Option<AllocatedPages> {
	allocate_pages_deferred(None, num_pages)
		.map(|(ap, _action)| ap)
		.ok()
}


/// Allocates pages with no constraints on the starting virtual address, 
/// with a size given by the number of bytes. 
/// 
/// This function still allocates whole pages by rounding up the number of bytes. 
/// See [`allocate_pages_deferred()`](fn.allocate_pages_deferred.html) for more details. 
pub fn allocate_pages_by_bytes(num_bytes: usize) -> Option<AllocatedPages> {
	allocate_pages_by_bytes_deferred(None, num_bytes)
		.map(|(ap, _action)| ap)
		.ok()
}


/// Allocates pages starting at the given `VirtualAddress` with a size given in number of bytes. 
/// 
/// This function still allocates whole pages by rounding up the number of bytes. 
/// See [`allocate_pages_deferred()`](fn.allocate_pages_deferred.html) for more details. 
pub fn allocate_pages_by_bytes_at(vaddr: VirtualAddress, num_bytes: usize) -> Result<AllocatedPages, &'static str> {
	allocate_pages_by_bytes_deferred(Some(vaddr), num_bytes)
		.map(|(ap, _action)| ap)
}


/// Allocates the given number of pages starting at the page containing the given `VirtualAddress`.
/// 
/// See [`allocate_pages_deferred()`](fn.allocate_pages_deferred.html) for more details. 
pub fn allocate_pages_at(vaddr: VirtualAddress, num_pages: usize) -> Result<AllocatedPages, &'static str> {
	allocate_pages_deferred(Some(vaddr), num_pages)
		.map(|(ap, _action)| ap)
}


/// Converts the page allocator from using static memory (a primitive array) to dynamically-allocated memory.
/// 
/// Call this function once heap allocation is available. 
/// Calling this multiple times is unnecessary but harmless, as it will do nothing after the first invocation.
#[doc(hidden)] 
pub fn convert_to_heap_allocated() {
	FREE_PAGE_LIST.lock().convert_to_heap_allocated();
}

/// A debugging function used to dump the full internal state of the page allocator. 
#[doc(hidden)] 
pub fn dump_page_allocator_state() {
	debug!("--------------- PAGE ALLOCATOR LIST ---------------");
	for c in FREE_PAGE_LIST.lock().iter() {
		debug!("{:X?}", c);
	}
	debug!("---------------------------------------------------");
}
