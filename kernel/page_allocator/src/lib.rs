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
#![feature(const_fn, const_in_array_repeat_expressions)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate kernel_config;
extern crate memory_structs;
extern crate spin;
extern crate intrusive_collections;
use intrusive_collections::Bound;


mod static_array_rb_tree;
// mod static_array_linked_list;


use core::{
	borrow::Borrow, 
	cmp::Ordering, 
	fmt, 
	ops::{Deref, DerefMut},
};
use kernel_config::memory::*;
use memory_structs::{VirtualAddress, Page, PageRange};
use spin::Mutex;
use static_array_rb_tree::*;


/// Regions that are pre-designated for special usage, specifically the kernel's initial identity mapping.
/// They will be allocated from if an address within them is specifically requested;
/// otherwise, they will only be allocated from as a "last resort" once all other non-designated address ranges are exhausted.
///
/// Any virtual addresses **less than or equal** to this address is considered "designated".
/// This lower part of the address range covers from 0x0 to the end of the kernel physical address.
/// 
// TODO: replace this with the dynamically-discovered end of the kernel identity mapping section (kernel_phys_end)
const DESIGNATED_PAGES_LOW_END: Page = Page::containing_address(VirtualAddress::new_canonical(0x40_0000 - 1));
/// Any virtual addresses **greater than or equal to** this address is considered "designated".
/// This higher part of the address range covers from the beginning of the heap area to the end of the address space.
// TODO: once the heap is fully dynamic and not dependent on constant addresses, we can move this up to KERNEL_TEXT_START (511th entry of P4).
const DESIGNATED_PAGES_HIGH_START: Page = Page::containing_address(VirtualAddress::new_canonical( KERNEL_HEAP_START));

static DESIGNATED_REGIONS: [PageRange; 2] = [
	PageRange::new(
		Page::containing_address(VirtualAddress::zero()), 
		DESIGNATED_PAGES_LOW_END,
	),
	// the higher part of the address range, kernel heap/offset to the end.
	PageRange::new(
		DESIGNATED_PAGES_HIGH_START,
		Page::containing_address(VirtualAddress::new_canonical(MAX_VIRTUAL_ADDRESS)),
	)
];

/// The single, system-wide list of free virtual memory pages.
/// Currently this list includes both free and allocated chunks of pages together in the same list,
/// but it may be better to separate them in the future,
/// especially when we transition to a RB-tree or a better data structure to track allocated pages. 
///
/// Because we use 510th entry of the top-level P4 page table for our recursive page table mapping,
/// we must never invlude the range of addresses covered by that entry.
/// Those forbidden addresses include the range from `0xFFFF_FF00_0000_0000` to `0xFFFF_FF80_0000_0000 - 1`.
/// All other possible virtual addresses are usable by the page allocator.
static FREE_PAGE_LIST: Mutex<StaticArrayRBTree<Chunk>> = Mutex::new(StaticArrayRBTree::new([

	// The first region contains all pages *below* the beginning of the 510th entry of P4. 
	// We split it up into three chunks just for ease, since it overlaps the designated regions.
	Some(Chunk { 
		allocated: false,
		pages: PageRange::new(
			Page::containing_address(VirtualAddress::zero()),
			DESIGNATED_PAGES_LOW_END,
		)
	}),
	Some(Chunk { 
		allocated: false,
		pages: PageRange::new(
			// This is just DESIGNATED_PAGES_LOW_END + 1, but written in a way that is const-compatible.
			Page::containing_address(VirtualAddress::new_canonical(DESIGNATED_PAGES_LOW_END.start_address().value() + PAGE_SIZE)),
			// This is just DESIGNATED_PAGES_HIGH_START - 1, but written in a way that is const-compatible.
			Page::containing_address(VirtualAddress::new_canonical(DESIGNATED_PAGES_HIGH_START.start_address().value() - PAGE_SIZE)),
		)
	}),
	Some(Chunk { 
		allocated: false,
		pages: PageRange::new(
			DESIGNATED_PAGES_HIGH_START,
			// End at the last page right beneath the beginning of the 510th entry of P4.
			Page::containing_address(VirtualAddress::new_canonical(KERNEL_TEXT_START - ADDRESSABILITY_PER_P4_ENTRY - 1)),
		)
	}),

	// The second region contains all pages *above* the end of the 510th entry of P4, i.e., starting at the 511th (last) entry of P4.
	// This is fully covered by the second (higher) designated region.
	Some(Chunk { 
		allocated: false,
		pages: PageRange::new(
			Page::containing_address(VirtualAddress::new_canonical(KERNEL_TEXT_START)),
			Page::containing_address(VirtualAddress::new_canonical(MAX_VIRTUAL_ADDRESS)),
		)
	}),
	None, None, None, None,
	None, None, None, None, None, None, None, None,
	None, None, None, None, None, None, None, None,
	None, None, None, None, None, None, None, None,
]));


/// A range of contiguous pages and whether they're allocated or free.
///
/// # Ordering and Equality
///
/// `Chunk` implements the `Ord` trait, and its total ordering is ONLY based on
/// its **starting** `Page`. This is useful so we can store `Chunk`s in a sorted collection.
///
/// Similarly, `Chunk` implements equality traits, `Eq` and `PartialEq`,
/// both of which are also based ONLY on the **starting** `Page` of the `Chunk`.
/// Thus, comparing two `Chunk`s with the `==` or `!=` operators may not work as expected.
/// since it ignores their allocated status and their actual range of pages.
#[derive(Debug, Clone, Eq)]
struct Chunk {
	/// Whether or not this Chunk is currently allocated. If false, it is free.
	allocated: bool,
	/// The Pages covered by this chunk, an inclusive range. 
	pages: PageRange,
}
impl Chunk {
	fn as_allocated_pages(&self) -> AllocatedPages {
		assert!(self.allocated, "BUG: Chunk {:?} wasn't marked as allocated", self);
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

impl Ord for Chunk {
    fn cmp(&self, other: &Self) -> Ordering {
        self.pages.start().cmp(other.pages.start())
    }
}

impl PartialOrd for Chunk {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Chunk {
    fn eq(&self, other: &Self) -> bool {
        self.pages.start() == other.pages.start()
    }
}

impl Borrow<Page> for &'_ Chunk {
	#[inline]
	fn borrow(&self) -> &Page {
		self.pages.start()
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
		write!(f, "AllocatedPages({:?})", self.pages)
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
			let first  = PageRange::new(*self.pages.start(), end_of_first);
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
	free_list: &'list Mutex<StaticArrayRBTree<Chunk>>,
	/// A reference to the list into which we will insert the allocated `Chunk`s.
	allocated_list: &'list Mutex<StaticArrayRBTree<Chunk>>,
	/// The chunk that was marked as allocated during the page allocation. 
	/// NOTE: we don't actually need to keep track of the list of allocated chunks, 
	/// but it's handy for debugging purposes and easy deallocation.
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
		// Insert all of the chunks, both allocated and free ones, into the list. 
		if self.free1.size_in_pages() > 0 {
			self.free_list.lock().insert(self.free1.clone()).unwrap();
		}
		if self.allocated.size_in_pages() > 0 {
			self.allocated_list.lock().insert(self.allocated.clone()).unwrap();
		}
		if self.free2.size_in_pages() > 0 {
			self.free_list.lock().insert(self.free2.clone()).unwrap();
		}
	}
}




fn find_specific_chunk<'list>(
	list: &'list mut StaticArrayRBTree<Chunk>,
	requested_page: Page,
	num_pages: usize
) -> Option<ValueRefMut<'list, Chunk>> {

	// The end page is an inclusive bound, hence the -1. Parentheses are needed to avoid overflow.
	let requested_end_page = requested_page + (num_pages - 1); 

	match &mut list.0 {
		Inner::Array(ref mut arr) => {
			for c in arr.iter_mut().flatten() {
				if requested_page >= *c.pages.start() && requested_end_page <= *c.pages.end() {
					// Here: chunk `c` was big enough and did contain the requested address.
					// If it's not allocated, we can use it. 
					if !c.allocated {
						return Some(ValueRefMut::Array(c));
					} else {
						error!("Page allocator: requested {}-page allocation at address {:?}, but address was already allocated.",
							num_pages, requested_page.start_address()
						);
						return None;
					}
				}
			}
		}
		Inner::RBTree(ref mut tree) => {
			let cursor_mut = tree.lower_bound_mut(Bound::Included(&requested_page));
			let c = if let Some(chunk) = cursor_mut.get().map(|w| w.deref()) {
				chunk
			}
			else {
				error!("Page allocator: requested {}-page allocation at address {:?}, but address was not found in free page list.",
					num_pages, requested_page.start_address()
				);
				return None;
			};

			if requested_page >= *c.pages.start() && requested_end_page <= *c.pages.end() {
				debug!("Found suitable chunk at {:?}", c);
				// Here: chunk `c` was big enough and did contain the requested address.
				// If it's not allocated, we can use it. 
				if !c.allocated {
					return Some(ValueRefMut::RBTree(cursor_mut));
				} else {
					error!("Page allocator: requested {}-page allocation at address {:?}, but address was already allocated.",
						num_pages, requested_page.start_address()
					);
					return None;
				}
			}
		}
	}

	None
}

fn find_any_chunk<'list>(
	list: &'list mut StaticArrayRBTree<Chunk>,
	num_pages: usize
) -> Option<ValueRefMut<'list, Chunk>> {
	// ignore designated regions at first
	{
		match list.0 {
			Inner::Array(ref mut arr) => {
				{
				for c in arr.iter_mut().flatten() {
					// Skip allocated chunks, too-small chunks, or chunks in the designated regions.
					if c.allocated || 
						c.size_in_pages() < num_pages || 
						c.pages.start() <= &DESIGNATED_PAGES_LOW_END || 
						c.pages.end() >= &DESIGNATED_PAGES_HIGH_START
					{
						continue;
					} 
					else {
						return Some(ValueRefMut::Array(c));
					}
				}
				}
			}
			Inner::RBTree(ref mut tree) => {
				// NOTE: if RBTree had a `range_mut()` method, we could simply do the following:
				// ```
				// let eligible_chunks = tree.range(
				// 	Bound::Excluded(&DESIGNATED_PAGES_LOW_END),
				// 	Bound::Excluded(&DESIGNATED_PAGES_HIGH_START)
				// );
				// for c in eligible_chunks { ... }
				// ```
				//
				// However, RBTree doesn't have a `range_mut()` method, so we use cursors for manual iteration.
				let mut cursor = tree.lower_bound_mut(Bound::Excluded(&DESIGNATED_PAGES_LOW_END));
				while let Some(chunk) = cursor.get().map(|w| w.deref()) {
					if (*chunk.pages.start() + num_pages) < DESIGNATED_PAGES_HIGH_START {
						break;
					}
					if !chunk.allocated && num_pages < chunk.size_in_pages() {
						return Some(ValueRefMut::RBTree(cursor));
					}
					cursor.move_next();
				}
			}
		};
	}

	// if we can't find any chunks at all, then look in both designated regions
	warn!("PageAllocator: untested scenario: non-designated chunks are all allocated, \
		  falling back to allocating {} pages from designated regions!", num_pages);
	match list.0 {
		Inner::Array(ref mut arr) => {
			for c in arr.iter_mut().flatten() {
				if !c.allocated && num_pages <= c.size_in_pages() {
					return Some(ValueRefMut::Array(c));
				}
			}
		}
		Inner::RBTree(ref mut tree) => {
			// NOTE: if RBTree had a `range_mut()` method, we could simply do the following:
			// ```
			// let eligible_chunks = tree.range(
			// 	Bound::<&Page>::Unbounded,
			// 	Bound::Included(&DESIGNATED_PAGES_LOW_END)
			// ).chain(tree.range(
			// 	Bound::Included(&DESIGNATED_PAGES_HIGH_START),
			// 	Bound::<&Page>::Unbounded
			// ));
			// for c in eligible_chunks { ... }
			// ```
			//
			// However, RBTree doesn't have a `range_mut()` method, so we use two sets of cursors for manual iteration.
			// The first cursor iterates over the lower designated region, from higher addresses to lower, down to zero.
			let mut cursor = tree.upper_bound_mut(Bound::Included(&DESIGNATED_PAGES_LOW_END));
			while let Some(chunk) = cursor.get().map(|w| w.deref()) {
				if !chunk.allocated && num_pages < chunk.size_in_pages() {
					return Some(ValueRefMut::RBTree(cursor));
				}
				cursor.move_prev();
			}

			// The second cursor iterates over the higher designated region, from lower addresses to higher, up to max.
			let mut cursor = tree.lower_bound_mut(Bound::Included(&DESIGNATED_PAGES_HIGH_START));
			while let Some(chunk) = cursor.get().map(|w| w.deref()) {
				if !chunk.allocated && num_pages < chunk.size_in_pages() {
					// return Some(ValueRefMut::RBTree(cursor));
				}
				cursor.move_next();
			}
		}
	}

	None
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

	debug!("PageAllocator: requesting {} pages starting at {:?}", num_pages, requested_vaddr);
	let mut locked_list = FREE_PAGE_LIST.lock();

	// First, find an appropriate chunk that can satisfy the allocation request.
	// An appropriate chunk satisfies the following conditions:
	// - Can fit the requested size (starting at the requested address) within the chunk
	// - The chunk can only be within in a designated region if a specific address was requested
	// - The chunk itself is not marked as allocated
	let mut chunk_ref = if let Some(vaddr) = requested_vaddr {
		find_specific_chunk(&mut locked_list, Page::containing_address(vaddr), num_pages)
			.ok_or_else(|| {
				error!("PageAllocator: failed to allocated {} pages at requested address {} because address was already allocated.",
					num_pages, vaddr
				);
				"requested address already allocated"
			})
	} else {
		find_any_chunk(&mut locked_list, num_pages)
			.ok_or_else(|| {
				error!("PageAllocator: out of address space, failed to allocated {} pages at any address", num_pages);
				"out of virtual address space"
			})
	}?;
	
	let c = chunk_ref.get().ok_or("BUG: suitable chunk found, but it had no value")?;
	let start_page = requested_vaddr.map(Page::containing_address).unwrap_or(*c.pages.start());

	// Second, mark the chunk as allocated and split the chunk as needed.

	// Easy optimization: if this chunk is exactly the right size, just update it in-place as 'allocated' and return that chunk.
	if num_pages == c.pages.size_in_pages() {
		let new_chunk = Chunk {
			allocated: true,
			pages: c.pages.clone(),
		};
		let new_alloc = new_chunk.as_allocated_pages();
		chunk_ref.replace_with(new_chunk).unwrap();
		return Ok((
			new_alloc,
			DeferredAllocAction::new(None, None, None),
		));
	}

	// The new allocated chunk might start in the middle of an existing chunk,
	// so we need to break up that existing chunk into 3 possible chunks: before, newly-allocated, and after.
	let new_allocation = Chunk {
		allocated: true,
		pages: PageRange::new(start_page, start_page + num_pages),
	};
	let before = Chunk {
		allocated: false,
		pages: PageRange::new(*c.pages.start(), *new_allocation.start() - 1),
	};
	let after = Chunk {
		allocated: false,
		pages: PageRange::new(*new_allocation.end() + 1, *c.pages.end()),
	};

	let deferred_action: DeferredAllocAction;
	// Since we're updating the chunk in-place here, we need to make sure we preserve the ordering of the free pages list. 
	// Thus, we set that chunk to be the `before` chunk, unless `before` is zero-sized, 
	// in which case we set that chunk to be `new_allocation`.
	if before.size_in_pages() > 0 {
		chunk_ref.replace_with(before).unwrap();
		deferred_action = DeferredAllocAction::new(
			new_allocation.clone(),
			after,
			None,
		);
	} else {
		chunk_ref.replace_with(new_allocation.clone()).unwrap();
		deferred_action = DeferredAllocAction::new(
			None, // we already set this chunk in-place to the newly-allocated chunk above
			before,
			after,
		);
	}

	Ok((new_allocation.as_allocated_pages(), deferred_action))
}


/// Similar to [`allocated_pages_deferred()`](fn.allocate_pages_deferred.html),
/// but accepts a size value for the allocated pages in number of bytes instead of number of pages. 
/// 
/// This function still allocates whole pages by rounding up the number of bytes. 
pub fn allocate_pages_by_bytes_deferred(
	requested_vaddr: Option<VirtualAddress>,
	num_bytes: usize,
) -> Result<(AllocatedPages, DeferredAllocAction<'static>), &'static str> {
	let actual_num_bytes = if let Some(vaddr) = requested_vaddr {
		num_bytes + (vaddr.value() % PAGE_SIZE)
	} else {
		num_bytes
	};
	let num_pages = (actual_num_bytes + PAGE_SIZE - 1) / PAGE_SIZE; // round up
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


/// Allocates the given number of pages starting at (inclusive of) the page containing the given `VirtualAddress`.
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
