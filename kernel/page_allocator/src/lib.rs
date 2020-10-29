//! Provides a (currently mediocre) allocator for virtual memory pages.
//! The minimum unit of allocation is a single page. 
//! 
//! This also supports early allocation of pages (up to 32 chunks) before heap allocation is available,
//! and does so behind the scenes using the same single interface. 
//! Once heap allocation is available, it uses a dynamically-allocated list of page chunks to track allocations.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate kernel_config;
extern crate memory_structs;
extern crate spin;

use alloc::collections::LinkedList;
use core::ops::Deref;
use kernel_config::memory::{KERNEL_TEXT_START, KERNEL_TEXT_MAX_SIZE, PAGE_SIZE};
use memory_structs::{VirtualAddress, Page, PageRange};
use spin::Mutex;

/// A group of contiguous pages, much like a hole in other allocators. 
#[derive(Debug)]
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
}


/// Represents an allocated range of virtual addresses, specified in pages. 
/// These pages are not initially mapped to any physical memory frames, you must do that separately.
/// This object represents ownership of those pages; if this object falls out of scope,
/// it will be dropped, and the pages will be de-allocated. 
/// See `MappedPages` struct for a similar object that unmaps pages when dropped.
#[derive(Debug)]
pub struct AllocatedPages {
	pages: PageRange,
}

impl Deref for AllocatedPages {
    type Target = PageRange;
    fn deref(&self) -> &PageRange {
        &self.pages
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

	// /// Splits this `AllocatedPages` into two separate `AllocatedPages` objects,
	// /// in which the first `AllocatedPages` object will 
	// pub fn split(self, starting_at_page: Page) -> Option<(AllocatedPages, AllocatedPages)> {
	// 	if self.contains(&starting_at_page) {
	// 		None 
	// 	} else {
	// 		None
	// 	}
	// }
}

// impl Drop for AllocatedPages {
//     fn drop(&mut self) {
// 		trace!("page_allocator: deallocate_pages is not yet implemented, trying to dealloc: {:?}", _pages);
// 		unimplemented!();
// 		Ok(())
//     }
// }


/// A convenience wrapper around a LinkedList and a primitive array
/// that allows the caller to create one statically in a const context, 
/// and then abstract over both when using it. 
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

	/// Returns an iterator over references to items in this collection.
	pub fn iter(&self) -> impl Iterator<Item = &T> {
		let mut iter_a = None;
		let mut iter_b = None;
		match self {
			StaticArrayLinkedList::Array(arr)     => iter_a = Some(arr.iter().flatten()),
			StaticArrayLinkedList::LinkedList(ll) => iter_b = Some(ll.iter()),
		}
		iter_a.into_iter().flatten().chain(iter_b.into_iter().flatten())
	}

	/// Returns an iterator over mutable references to items in this collection.
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

/// The single, system-wide list of free virtual memory pages.
/// Currently this list includes both free and allocated chunks of pages together in the same list,
/// but it may be better 
static FREE_PAGE_LIST: Mutex<StaticArrayLinkedList<Chunk>> = Mutex::new(StaticArrayLinkedList::Array([
	// The list of available pages starts as one big chunk that spans the entire virtual address space. 
	Some(Chunk { 
		allocated: false,
		pages: PageRange::new(
			Page::containing_address(VirtualAddress::new_canonical(KERNEL_TEXT_START)),
			Page::containing_address(VirtualAddress::new_canonical(KERNEL_TEXT_START + KERNEL_TEXT_MAX_SIZE)),
		)
	}),
	None, None, None, None, None, None, None,
	None, None, None, None, None, None, None, None,
	None, None, None, None, None, None, None, None,
	None, None, None, None, None, None, None, None,
]));


/// Convenience function for allocating pages by giving the number of bytes
/// rather than the number of pages. 
/// This function still allocates whole pages by rounding up the number of bytes. 
/// See [`allocate_pages()`](fn.allocate_pages.html)
pub fn allocate_pages_by_bytes(num_bytes: usize) -> Option<AllocatedPages> {
	let num_pages = (num_bytes + PAGE_SIZE - 1) / PAGE_SIZE; // round up
	allocate_pages(num_pages)
}


/// Allocates the given number of pages, but simply reserves the virtual addresses; 
/// it does not allocate actual physical memory frames nor do any mapping. 
/// Thus these pages aren't directly usable until they are mapped to physical frames. 
/// Allocation is quick, technically O(n) but generally will allocate immediately
/// because the largest free chunks are stored at the front of the list.
/// Fragmentation isn't cleaned up until we're out of address space, but not really a big deal.
pub fn allocate_pages(num_pages: usize) -> Option<AllocatedPages> {

	if num_pages == 0 {
		warn!("allocate_pages(): requested an allocation of 0 pages... stupid!");
		return None;
	}

	// the Pages holding the chunk to be allocated, if we can find one.
	let mut allocated_page_range: Option<PageRange> = None;

	let mut locked_list = FREE_PAGE_LIST.lock();
	for mut c in locked_list.iter_mut() {
		// skip already-allocated chunks and chunks that are too small
		if c.allocated || c.pages.size_in_pages() < num_pages {
			continue;
		}

		// Here: we have found a suitable chunk.
		// If the chunk is exactly the right size, just update it in-place as 'allocated' and return that chunk.
		if c.pages.size_in_pages() == num_pages {
			c.allocated = true;
			return Some(c.as_allocated_pages())
		}
		
		// Here: we have a suitable chunk, we need to split it up into two chunks: an allocated one and a free one. 
		let new_allocation = PageRange::with_num_pages(*c.pages.start(), num_pages);
		// First, update in-place the original free (unallocated) chunk to be smaller, 
		// since we're removing pages from the beginning of it.
		c.pages = PageRange::new(*new_allocation.end() + 1, *c.pages.end());

		// Second, create a new chunk that has the pages we've peeled off of the beginning of the original chunk.
		// (or rather, we create the chunk below outside of the iterator loop, so here we just tell it where to start)
		allocated_page_range = Some(new_allocation);
		break;
	}

	if let Some(pr) = allocated_page_range {
		let new_chunk = Chunk {
			allocated: true,
			pages: pr
		};
		let ret = new_chunk.as_allocated_pages();
		locked_list.push_back(new_chunk).unwrap();
		Some(ret)
	}
	else {
		error!("page_allocator: out of virtual address space."); 
		return None;
	}
}


/// Convenience function for allocating pages at the given virtual address by 
/// giving the number of bytes rather than the number of pages. 
/// This function still allocates whole pages by rounding up the number of bytes. 
/// See [`allocate_pages_at()`](fn.allocate_pages_at.html)
pub fn allocate_pages_by_bytes_at(vaddr: VirtualAddress, num_bytes: usize) -> Result<AllocatedPages, &'static str> {
	let num_pages = (num_bytes + PAGE_SIZE - 1) / PAGE_SIZE; // round up
	allocate_pages_at(vaddr, num_pages)
}


/// Allocates the given number of pages starting at the page containing the given `VirtualAddress`.
/// This simply reserves the virtual addresses, but does not allocate actual physical memory frames nor do any mapping. 
/// Thus these pages aren't directly usable until they are mapped to physical frames. 
/// 
/// Returns an error is out of virtual address space, or if the specified virtual address is already allocated.
pub fn allocate_pages_at(vaddr: VirtualAddress, num_pages: usize) -> Result<AllocatedPages, &'static str> {
	if num_pages == 0 {
		warn!("allocate_pages(): requested an allocation of 0 pages... stupid!");
		return Err("cannot allocate zero pages");
	}

	let desired_start_page = Page::containing_address(vaddr);

	// the Pages holding the chunk to be allocated, if we can find one.
	let mut allocated_page_range: Option<PageRange> = None;
	// the extra unallocated pages to be added to the list.
	let mut extra_free_chunk: Option<PageRange> = None;

	let mut locked_list = FREE_PAGE_LIST.lock();
	for c in locked_list.iter_mut() {
		// Look for the chunk that contains the desired page
		if c.pages.contains(&desired_start_page) && c.pages.contains(&(desired_start_page + num_pages)) {
			if c.allocated {
				return Err("address already allocated");
			}
		}
		
		// The new allocated chunk might start in the middle of an existing chunk,
		// so we need to break up that existing chunk into 3 possible chunks: before, newly-allocated, and after.
		let new_allocation = PageRange::with_num_pages(desired_start_page, num_pages);
		let before = PageRange::new(*c.pages.start(), *new_allocation.start() - 1);
		let after  = PageRange::new(*new_allocation.end() + 1, *c.pages.end());

		// Put the larger free chunk in-place here, and the smaller one at the end, ignoring either if they are zero-sized.
		if before.size_in_pages() > after.size_in_pages() {
			if before.size_in_pages() > 0 {
				c.pages = before;
			}
			extra_free_chunk = Some(after);
		} else {
			if after.size_in_pages() > 0 {
				c.pages = after;
			}
			extra_free_chunk = Some(before);
		}

		allocated_page_range = Some(new_allocation);
		break;
	}

	if let Some(free_pages) = extra_free_chunk {
		if free_pages.size_in_pages() > 0 {
			locked_list.push_back(Chunk {
				allocated: false,
				pages: free_pages,
			}).unwrap();
		}
	}

	if let Some(pr) = allocated_page_range {
		let new_chunk = Chunk {
			allocated: true,
			pages: pr,
		};
		let ret = new_chunk.as_allocated_pages();
		locked_list.push_back(new_chunk).unwrap();
		Ok(ret)
	}
	else {
		error!("page_allocator: out of virtual address space."); 
		Err("out of virtual memory")
	}
}

/// Converts the page allocator from using static memory (a primitive array) to dynamically-allocated memory.
/// 
/// Call this function once heap allocation is available. 
pub fn convert_to_heap_allocated() {
	FREE_PAGE_LIST.lock().convert_to_heap_allocated();
}

/// A debugging function used to dump the full internal state of the page allocator. 
pub fn dump_page_allocator_state() {
	debug!("--------------- PAGE ALLOCATOR LIST ---------------");
	for c in FREE_PAGE_LIST.lock().iter() {
		debug!("{:X?}", c);
	}
	debug!("---------------------------------------------------");
}