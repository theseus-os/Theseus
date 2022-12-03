//! Provides an allocator for virtual memory pages.
//! The minimum unit of allocation is a single page. 
//! 
//! This also supports early allocation of pages (up to 32 separate chunks)
//! before heap allocation is available, and does so behind the scenes using the same single interface. 
//! 
//! Once heap allocation is available, it uses a dynamically-allocated list of page chunks to track allocations.
//! 
//! The core allocation function is [`allocate_pages_deferred()`](fn.allocate_pages_deferred.html), 
//! but there are several convenience functions that offer simpler interfaces for general usage. 
//!
//! # Notes and Missing Features
//! This allocator currently does **not** merge freed chunks (de-fragmentation) upon deallocation. 
//! It only merges free chunks lazily upon request, i.e., when we run out of address space
//! or when a requested address is in a chunk that needs to be merged with a nearby chunk.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate kernel_config;
extern crate memory_structs;
extern crate spin;
#[macro_use] extern crate static_assertions;
extern crate intrusive_collections;
use intrusive_collections::Bound;


mod static_array_rb_tree;
// mod static_array_linked_list;


use core::{borrow::Borrow, cmp::Ordering, fmt, ops::{Deref, DerefMut}};
use kernel_config::memory::*;
use memory_structs::{VirtualAddress, Page, PageRange};
use spin::{Mutex, Once};
use static_array_rb_tree::*;


/// Certain regions are pre-designated for special usage, specifically the kernel's initial identity mapping.
/// They will be allocated from if an address within them is specifically requested;
/// otherwise, they will only be allocated from as a "last resort" if all other non-designated address ranges are exhausted.
///
/// Any virtual addresses **less than or equal** to this address are considered "designated".
/// This lower part of the address range that's designated covers from 0x0 to this address.
static DESIGNATED_PAGES_LOW_END: Once<Page> = Once::new();

/// Defines the upper part of the address space that's designated, similar to `DESIGNATED_PAGES_LOW_END`. 
/// Any virtual addresses **greater than or equal to** this address is considered "designated".
/// This higher part of the address range covers from the beginning of the heap area to the end of the address space.
///
/// TODO: once the heap is fully dynamic and not dependent on constant addresses, we can move this up to KERNEL_TEXT_START (511th entry of P4).
static DESIGNATED_PAGES_HIGH_START: Page = Page::containing_address(VirtualAddress::new_canonical(KERNEL_HEAP_START));

const MIN_PAGE: Page = Page::containing_address(VirtualAddress::zero());
const MAX_PAGE: Page = Page::containing_address(VirtualAddress::new_canonical(MAX_VIRTUAL_ADDRESS));

/// The single, system-wide list of free chunks of virtual memory pages.
static FREE_PAGE_LIST: Mutex<StaticArrayRBTree<Chunk>> = Mutex::new(StaticArrayRBTree::empty());


/// Initialize the page allocator.
///
/// # Arguments
/// * `end_vaddr_of_low_designated_region`: the `VirtualAddress` that marks the end of the 
///   lower designated region, which should be the ending address of the initial kernel image
///   (a lower-half identity address).
/// 
/// The page allocator will only allocate addresses lower than `end_vaddr_of_low_designated_region`
/// if specifically requested.
/// General allocation requests for any virtual address will not use any address lower than that,
/// unless the rest of the entire virtual address space is already in use.
///
pub fn init(end_vaddr_of_low_designated_region: VirtualAddress) -> Result<(), &'static str> {
	assert!(end_vaddr_of_low_designated_region < DESIGNATED_PAGES_HIGH_START.start_address());
	let designated_low_end = DESIGNATED_PAGES_LOW_END.call_once(|| Page::containing_address(end_vaddr_of_low_designated_region));
	let designated_low_end = *designated_low_end;

	let initial_free_chunks = [
		// The first region contains all pages *below* the beginning of the 510th entry of P4. 
		// We split it up into three chunks just for ease, since it overlaps the designated regions.
		Some(Chunk { 
			pages: PageRange::new(
				Page::containing_address(VirtualAddress::zero()),
				designated_low_end,
			)
		}),
		Some(Chunk { 
			pages: PageRange::new(
				designated_low_end + 1,
				DESIGNATED_PAGES_HIGH_START - 1,
			)
		}),
		Some(Chunk { 
			pages: PageRange::new(
				DESIGNATED_PAGES_HIGH_START,
				// This is the page right below the beginning of the 510th entry of the top-level P4 page table.
				Page::containing_address(VirtualAddress::new_canonical(KERNEL_TEXT_START - ADDRESSABILITY_PER_P4_ENTRY - 1)),
			)
		}),

		// The second region contains all pages *above* the end of the 510th entry of P4, i.e., starting at the 511th (last) entry of P4.
		// This is fully covered by the second (higher) designated region.
		Some(Chunk { 
			pages: PageRange::new(
				Page::containing_address(VirtualAddress::new_canonical(KERNEL_TEXT_START)),
				Page::containing_address(VirtualAddress::new_canonical(MAX_VIRTUAL_ADDRESS)),
			)
		}),
		None, None, None, None,
		None, None, None, None, None, None, None, None,
		None, None, None, None, None, None, None, None,
		None, None, None, None, None, None, None, None,
	];

	*FREE_PAGE_LIST.lock() = StaticArrayRBTree::new(initial_free_chunks);
	Ok(())
}


/// A range of contiguous pages.
///
/// # Ordering and Equality
///
/// `Chunk` implements the `Ord` trait, and its total ordering is ONLY based on
/// its **starting** `Page`. This is useful so we can store `Chunk`s in a sorted collection.
///
/// Similarly, `Chunk` implements equality traits, `Eq` and `PartialEq`,
/// both of which are also based ONLY on the **starting** `Page` of the `Chunk`.
/// Thus, comparing two `Chunk`s with the `==` or `!=` operators may not work as expected.
/// since it ignores their actual range of pages.
#[derive(Debug, Clone, Eq)]
struct Chunk {
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
pub struct AllocatedPages {
	pages: PageRange,
}

// AllocatedPages must not be Cloneable, and it must not expose its inner pages as mutable.
assert_not_impl_any!(AllocatedPages: DerefMut, Clone);

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
    /// This function follows the behavior of [`core::slice::split_at()`],
    /// thus, either one of the returned `AllocatedPages` objects may be empty. 
    /// * If `at_page == self.start`, the first returned `AllocatedPages` object will be empty.
    /// * If `at_page == self.end + 1`, the second returned `AllocatedPages` object will be empty.
    /// 
    /// Returns an `Err` containing this `AllocatedPages` if `at_page` is otherwise out of bounds.
	/// 
    /// [`core::slice::split_at()`]: https://doc.rust-lang.org/core/primitive.slice.html#method.split_at
    pub fn split(self, at_page: Page) -> Result<(AllocatedPages, AllocatedPages), AllocatedPages> {
        let end_of_first = at_page - 1;

        let (first, second) = if at_page == *self.start() && at_page <= *self.end() {
            let first  = PageRange::empty();
            let second = PageRange::new(at_page, *self.end());
            (first, second)
        } 
        else if at_page == (*self.end() + 1) && end_of_first >= *self.start() {
            let first  = PageRange::new(*self.start(), *self.end()); 
            let second = PageRange::empty();
            (first, second)
        }
        else if at_page > *self.start() && end_of_first <= *self.end() {
            let first  = PageRange::new(*self.start(), end_of_first);
            let second = PageRange::new(at_page, *self.end());
            (first, second)
        }
        else {
            return Err(self);
        };

        // ensure the original AllocatedPages doesn't run its drop handler and free its pages.
        core::mem::forget(self);   
        Ok((
            AllocatedPages { pages: first }, 
            AllocatedPages { pages: second },
        ))
    }
}

impl Drop for AllocatedPages {
    fn drop(&mut self) {
		if self.size_in_pages() == 0 { return; }
		// trace!("page_allocator: deallocating {:?}", self);

		// Simply add the newly-deallocated chunk to the free pages list.
		let mut locked_list = FREE_PAGE_LIST.lock();
		let res = locked_list.insert(Chunk {
			pages: self.pages.clone(),
		});
		match res {
			Ok(_inserted_free_chunk) => return,
			Err(c) => error!("BUG: couldn't insert deallocated chunk {:?} into free page list", c),
		}
		
		// Here, we could optionally use above `_inserted_free_chunk` to merge the adjacent (contiguous) chunks
		// before or after the newly-inserted free chunk. 
		// However, there's no *need* to do so until we actually run out of address space or until 
		// a requested address is in a chunk that needs to be merged.
		// Thus, for performance, we save that for those future situations.
    }
}



/// A series of pending actions related to page allocator bookkeeping,
/// which may result in heap allocation. 
/// 
/// The actions are triggered upon dropping this struct. 
/// This struct can be returned from the `allocate_pages()` family of functions 
/// in order to allow the caller to precisely control when those actions 
/// that may result in heap allocation should occur. 
/// Such actions include adding chunks to lists of free pages or pages in use. 
/// 
/// The vast majority of use cases don't  care about such precise control, 
/// so you can simply drop this struct at any time or ignore it
/// with a `let _ = ...` binding to instantly drop it. 
pub struct DeferredAllocAction<'list> {
	/// A reference to the list into which we will insert the free `Chunk`s.
	free_list: &'list Mutex<StaticArrayRBTree<Chunk>>,
	/// A free chunk that needs to be added back to the free list.
	free1: Chunk,
	/// Another free chunk that needs to be added back to the free list.
	free2: Chunk,
}
impl<'list> DeferredAllocAction<'list> {
	fn new<F1, F2>(free1: F1, free2: F2) -> DeferredAllocAction<'list> 
		where F1: Into<Option<Chunk>>,
			  F2: Into<Option<Chunk>>,
	{
		let free_list = &FREE_PAGE_LIST;
		let free1 = free1.into().unwrap_or_else(Chunk::empty);
		let free2 = free2.into().unwrap_or_else(Chunk::empty);
		DeferredAllocAction { free_list, free1, free2 }
	}
}
impl<'list> Drop for DeferredAllocAction<'list> {
	fn drop(&mut self) {
		// Insert all of the chunks, both allocated and free ones, into the list. 
		if self.free1.size_in_pages() > 0 {
			self.free_list.lock().insert(self.free1.clone()).unwrap();
		}
		if self.free2.size_in_pages() > 0 {
			self.free_list.lock().insert(self.free2.clone()).unwrap();
		}
	}
}


/// Possible allocation errors.
#[derive(Debug)]
enum AllocationError {
	/// The requested address was not free: it was already allocated, or is outside the range of this allocator.
	AddressNotFree(Page, usize),
	/// The address space was full, or there was not a large-enough chunk 
	/// or enough remaining chunks that could satisfy the requested allocation size.
	OutOfAddressSpace(usize),
	/// The allocator has not yet been initialized.
	NotInitialized,
}
impl From<AllocationError> for &'static str {
	fn from(alloc_err: AllocationError) -> &'static str {
		match alloc_err {
			AllocationError::AddressNotFree(..) => "address was in use or outside of this page allocator's range",
			AllocationError::OutOfAddressSpace(..) => "out of virtual address space",
			AllocationError::NotInitialized => "the page allocator has not yet been initialized",
		}
	}
}


/// Searches the given `list` for the chunk that contains the range of pages from
/// `requested_page` to `requested_page + num_pages`.
fn find_specific_chunk(
	list: &mut StaticArrayRBTree<Chunk>,
	requested_page: Page,
	num_pages: usize
) -> Result<(AllocatedPages, DeferredAllocAction<'static>), AllocationError> {

	// The end page is an inclusive bound, hence the -1. Parentheses are needed to avoid overflow.
	let requested_end_page = requested_page + (num_pages - 1); 

	match &mut list.0 {
		Inner::Array(ref mut arr) => {
			for elem in arr.iter_mut() {
				if let Some(chunk) = elem {
					if requested_page >= *chunk.start() && requested_end_page <= *chunk.end() {
						// Here: `chunk` was big enough and did contain the requested address.
						return adjust_chosen_chunk(requested_page, num_pages, &chunk.clone(), ValueRefMut::Array(elem));
					}
				}
			}
		}
		Inner::RBTree(ref mut tree) => {
			let mut cursor_mut = tree.upper_bound_mut(Bound::Included(&requested_page));
			if let Some(chunk) = cursor_mut.get().map(|w| w.deref()) {
				if requested_page >= *chunk.start() {
					if requested_end_page <= *chunk.end() {
						return adjust_chosen_chunk(requested_page, num_pages, &chunk.clone(), ValueRefMut::RBTree(cursor_mut));
					} else {
						// Here, we've found a chunk that includes the requested start page, but it's too small
						// to cover the number of requested pages. 
						// Thus, we attempt to merge this chunk with the next contiguous chunk(s) to create one single larger chunk.
						let chunk = chunk.clone(); // ends the above borrow on `cursor_mut`
						let mut new_end_page = *chunk.end();
						cursor_mut.move_next();
						while let Some(next_chunk) = cursor_mut.get().map(|w| w.deref()) {
							if *next_chunk.start() - 1 == new_end_page {
								new_end_page = *next_chunk.end();
								cursor_mut.remove().expect("BUG: page_allocator failed to merge contiguous chunks.");
								// The above call to `cursor_mut.remove()` advances the cursor to the next chunk.
							} else {
								break; // the next chunk wasn't contiguous, so stop iterating.
							}
						}

						if new_end_page > *chunk.end() {
							cursor_mut.move_prev(); // move the cursor back to the original chunk
							let _removed_chunk = cursor_mut.replace_with(Wrapper::new_link(Chunk { pages: PageRange::new(*chunk.start(), new_end_page) }))
								.expect("BUG: page_allocator failed to replace the current chunk while merging contiguous chunks.");
							return adjust_chosen_chunk(requested_page, num_pages, &chunk, ValueRefMut::RBTree(cursor_mut));
						}
					}
				}
			}
		}
	}

	Err(AllocationError::AddressNotFree(requested_page, num_pages))
}


/// Searches the given `list` for any chunk large enough to hold at least `num_pages`.
///
/// It first attempts to find a suitable chunk **not** in the designated regions,
/// and only allocates from the designated regions as a backup option.
fn find_any_chunk<'list>(
	list: &'list mut StaticArrayRBTree<Chunk>,
	num_pages: usize
) -> Result<(AllocatedPages, DeferredAllocAction<'static>), AllocationError> {
	let designated_low_end = DESIGNATED_PAGES_LOW_END.get().ok_or(AllocationError::NotInitialized)?;

	// During the first pass, we ignore designated regions.
	match list.0 {
		Inner::Array(ref mut arr) => {
			for elem in arr.iter_mut() {
				if let Some(chunk) = elem {
					// Skip chunks that are too-small or in the designated regions.
					if  chunk.size_in_pages() < num_pages || 
						chunk.start() <= &designated_low_end || 
						chunk.end() >= &DESIGNATED_PAGES_HIGH_START
					{
						continue;
					} 
					else {
						return adjust_chosen_chunk(*chunk.start(), num_pages, &chunk.clone(), ValueRefMut::Array(elem));
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
			//
			// Because we allocate new pages by peeling them off from the beginning part of a chunk, 
			// it's MUCH faster to start the search for free pages from higher addresses moving down. 
			// This results in an O(1) allocation time in the general case, until all address ranges are already in use.
			let mut cursor = tree.upper_bound_mut(Bound::Excluded(&DESIGNATED_PAGES_HIGH_START));
			while let Some(chunk) = cursor.get().map(|w| w.deref()) {
				if chunk.start() <= &designated_low_end {
					break; // move on to searching through the designated regions
				}
				if num_pages < chunk.size_in_pages() {
					return adjust_chosen_chunk(*chunk.start(), num_pages, &chunk.clone(), ValueRefMut::RBTree(cursor));
				}
				warn!("Page allocator: unlikely scenario: had to search multiple chunks while trying to allocate {} pages at any address.", num_pages);
				cursor.move_prev();
			}
		}
	}

	// If we can't find any suitable chunks in the non-designated regions, then look in both designated regions.
	warn!("PageAllocator: unlikely scenario: non-designated chunks are all allocated, \
		  falling back to allocating {} pages from designated regions!", num_pages);
	match list.0 {
		Inner::Array(ref mut arr) => {
			for elem in arr.iter_mut() {
				if let Some(chunk) = elem {
					if num_pages <= chunk.size_in_pages() {
						return adjust_chosen_chunk(*chunk.start(), num_pages, &chunk.clone(), ValueRefMut::Array(elem));
					}
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
			let mut cursor = tree.upper_bound_mut(Bound::Included(designated_low_end));
			while let Some(chunk) = cursor.get().map(|w| w.deref()) {
				if num_pages < chunk.size_in_pages() {
					return adjust_chosen_chunk(*chunk.start(), num_pages, &chunk.clone(), ValueRefMut::RBTree(cursor));
				}
				cursor.move_prev();
			}

			// The second cursor iterates over the higher designated region, from the highest (max) address down to the designated region boundary.
			let mut cursor = tree.upper_bound_mut::<Chunk>(Bound::Unbounded);
			while let Some(chunk) = cursor.get().map(|w| w.deref()) {
				if chunk.start() < &DESIGNATED_PAGES_HIGH_START {
					// we already iterated over non-designated pages in the first match statement above, so we're out of memory. 
					break; 
				}
				if num_pages < chunk.size_in_pages() {
					return adjust_chosen_chunk(*chunk.start(), num_pages, &chunk.clone(), ValueRefMut::RBTree(cursor));
				}
				cursor.move_prev();
			}
		}
	}

	Err(AllocationError::OutOfAddressSpace(num_pages))
}


/// The final part of the main allocation routine. 
///
/// The given chunk is the one we've chosen to allocate from. 
/// This function breaks up that chunk into multiple ones and returns an `AllocatedPages` 
/// from (part of) that chunk, ranging from `start_page` to `start_page + num_pages`.
fn adjust_chosen_chunk(
	start_page: Page,
	num_pages: usize,
	chosen_chunk: &Chunk,
	mut chosen_chunk_ref: ValueRefMut<Chunk>,
) -> Result<(AllocatedPages, DeferredAllocAction<'static>), AllocationError> {

	// The new allocated chunk might start in the middle of an existing chunk,
	// so we need to break up that existing chunk into 3 possible chunks: before, newly-allocated, and after.
	//
	// Because Pages and VirtualAddresses use saturating add and subtract, we need to double-check that we're not creating
	// an overlapping duplicate Chunk at either the very minimum or the very maximum of the address space.
	let new_allocation = Chunk {
		// The end page is an inclusive bound, hence the -1. Parentheses are needed to avoid overflow.
		pages: PageRange::new(start_page, start_page + (num_pages - 1)),
	};
	let before = if start_page == MIN_PAGE {
		None
	} else {
		Some(Chunk {
			pages: PageRange::new(*chosen_chunk.start(), *new_allocation.start() - 1),
		})
	};
	let after = if new_allocation.end() == &MAX_PAGE { 
		None
	} else {
		Some(Chunk {
			pages: PageRange::new(*new_allocation.end() + 1, *chosen_chunk.end()),
		})
	};

	// some sanity checks -- these can be removed or disabled for better performance
	if let Some(ref b) = before {
		assert!(!new_allocation.contains(b.end()));
		assert!(!b.contains(new_allocation.start()));
	}
	if let Some(ref a) = after {
		assert!(!new_allocation.contains(a.start()));
		assert!(!a.contains(new_allocation.end()));
	}

	// Remove the chosen chunk from the free page list.
	let _removed_chunk = chosen_chunk_ref.remove();
	assert_eq!(Some(chosen_chunk), _removed_chunk.as_ref()); // sanity check

	// TODO: Re-use the allocated wrapper if possible, rather than allocate a new one entirely.
	// if let RemovedValue::RBTree(Some(wrapper_adapter)) = _removed_chunk { ... }

	Ok((
		new_allocation.as_allocated_pages(),
		DeferredAllocAction::new(before, after),
	))
}


/// The core page allocation routine that allocates the given number of virtual pages,
/// optionally at the requested starting `VirtualAddress`.
/// 
/// This simply reserves a range of virtual addresses, it does not allocate 
/// actual physical memory frames nor do any memory mapping. 
/// Thus, the returned `AllocatedPages` aren't directly usable until they are mapped to physical frames. 
/// 
/// Allocation is based on a red-black tree and is thus `O(log(n))`.
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

	let mut locked_list = FREE_PAGE_LIST.lock();

	// The main logic of the allocator is to find an appropriate chunk that can satisfy the allocation request.
	// An appropriate chunk satisfies the following conditions:
	// - Can fit the requested size (starting at the requested address) within the chunk.
	// - The chunk can only be within in a designated region if a specific address was requested, 
	//   or all other non-designated chunks are already in use.
	if let Some(vaddr) = requested_vaddr {
		find_specific_chunk(&mut locked_list, Page::containing_address(vaddr), num_pages)
	} else {
		find_any_chunk(&mut locked_list, num_pages)
	}.map_err(From::from) // convert from AllocationError to &str
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
	debug!("--------------- FREE PAGES LIST ---------------");
	for c in FREE_PAGE_LIST.lock().iter() {
		debug!("{:X?}", c);
	}
	debug!("---------------------------------------------------");
}
