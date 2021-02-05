

use core::{cmp::Ordering, fmt, ops::Deref, ops::DerefMut};
use kernel_config::memory::*;
use memory_structs::{VirtualAddress, Page, PageRange};
use spin::Mutex;

use alloc::boxed::Box;
use intrusive_collections::{
    intrusive_adapter,
    rbtree::{RBTree, CursorMut},
    RBTreeLink,
	KeyAdapter,
};

const MIN_PAGE: Page = Page::containing_address(VirtualAddress::new_canonical(0x2000_0000));
const MAX_PAGE: Page = Page::containing_address(VirtualAddress::new_canonical(0x2002_0000));

/// A wrapper for the type stored in the `StaticArrayRBTree::Inner::RBTree` variant.
pub struct Wrapper<T: Ord> {
    link: RBTreeLink,
    inner: T,
}
intrusive_adapter!(WrapperAdapter<T> = Box<Wrapper<T>>: Wrapper<T> { link: RBTreeLink } where T: Ord);

// Use the inner type `T` (which must implement `Ord`) to define the key
// for properly ordering the elements in the RBTree.
impl<'a, T: Ord + 'a> KeyAdapter<'a> for WrapperAdapter<T> {
    type Key = &'a T;
    fn get_key(&self, value: &'a Wrapper<T>) -> Self::Key {
        &value.inner
    }
}
impl <T: Ord> Deref for Wrapper<T> {
	type Target = T;
	fn deref(&self) -> &T {
		&self.inner
	}
}
impl <T: Ord> DerefMut for Wrapper<T> {
	fn deref_mut(&mut self) -> &mut T {
		&mut self.inner
	}
}
impl <T: Ord> Wrapper<T> {
    /// Convenience method for creating a new link
    fn new_link(value: T) -> Box<Self> {
        Box::new(Wrapper {
            link: RBTreeLink::new(),
            inner: value,
        })
    }
}

struct MemChunkRBTree<T: Ord>(RBTree<WrapperAdapter<T>>);

impl<T: Ord + 'static> MemChunkRBTree<T> {
    /// Push the given `value` into this collection.
	pub fn insert(&mut self, value: T) {
		self.0.insert(Wrapper::new_link(value));
	}
}

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
	fn into_allocated_pages(self) -> AllocatedPages {
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
	free_list: &'list Mutex<MemChunkRBTree<Chunk>>,
	/// A reference to the list into which we will insert the allocated `Chunk`s.
	allocated_list: &'list Mutex<MemChunkRBTree<Chunk>>,
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
		let free_list = &MEM_CHUNK_TREE;
		let allocated_list = &MEM_CHUNK_TREE;
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
			self.free_list.lock().insert(self.free1.clone());
		}
		if self.allocated.size_in_pages() > 0 {
			self.allocated_list.lock().insert(self.allocated.clone());
		}
		if self.free2.size_in_pages() > 0 {
			self.free_list.lock().insert(self.free2.clone());
		}
	}
}


#[allow(dead_code)]
/// Possible allocation errors.
enum AllocationError {
	/// The requested address was already allocated.
	AddressInUse(Page, usize),
	/// The requested address was outside of the range of this allocator. 
	AddressOutOfRange(Page, usize),
	/// The address space was full, or there was not a large-enough chunk 
	/// or enough remaining chunks that could satisfy the requested allocation size.
	OutOfAddressSpace(usize),
}
impl From<AllocationError> for &'static str {
	fn from(alloc_err: AllocationError) -> &'static str {
		match alloc_err {
			AllocationError::AddressInUse(..) => "requested address was already allocated",
			AllocationError::AddressOutOfRange(..) => "address was outside of this allocator's range",
			AllocationError::OutOfAddressSpace(..) => "out of address space",
		}
	}
}

struct ValueRefMut<'list, T: Ord> (
	&'list mut CursorMut<'list, WrapperAdapter<T>>
);
impl <'list, T: Ord> ValueRefMut<'list, T> {
	pub fn replace_with(&mut self, new_value: T) -> Result<(), T> {
		self.0.replace_with(Wrapper::new_link(new_value))
			.map_err(|e| (*e).inner)?;
		Ok(())
	}
}

lazy_static! {
	static ref MEM_CHUNK_TREE: Mutex<MemChunkRBTree<Chunk>> = {
		let mut tree = RBTree::new(WrapperAdapter::new());
		let allocatable_start = VirtualAddress::new(0x2000_c000).unwrap();
		let allocatable_end = VirtualAddress::new(0x2001_7fff).unwrap();
		tree.insert(Wrapper::new_link(Chunk{
			allocated: false,
			pages: PageRange::new(Page::containing_address(allocatable_start), Page::containing_address(allocatable_end))
		}));
		Mutex::new(MemChunkRBTree(tree))
	};
}


/// Searches the given `list` for any chunk large enough to hold at least `num_pages`.
///
/// It first attempts to find a suitable chunk **not** in the designated regions,
/// and only allocates from the designated regions as a backup option.
fn find_any_chunk<'list>(
	tree: &'list mut MemChunkRBTree<Chunk>,
	num_pages: usize
) -> Result<(AllocatedPages, DeferredAllocAction<'static>), AllocationError> {
    let tree = &mut tree.0;
    let mut cursor = tree.front_mut();
    while let Some(chunk) = cursor.get().map(|w| w.deref()) {
        if (*chunk.pages.start() + num_pages) > MAX_PAGE { // Use greater than (not >=) because ranges are inclusive
            break;
        }
        if !chunk.allocated && num_pages < chunk.size_in_pages() {
            return adjust_chosen_chunk(*chunk.start(), num_pages, &chunk.clone(), ValueRefMut(&mut cursor));
        }
        cursor.move_next();
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
		allocated: true,
		// The end page is an inclusive bound, hence the -1. Parentheses are needed to avoid overflow.
		pages: PageRange::new(start_page, start_page + (num_pages - 1)),
	};
	let before = if start_page == MIN_PAGE {
		None
	} else {
		Some(Chunk {
			allocated: false,
			pages: PageRange::new(*chosen_chunk.pages.start(), *new_allocation.start() - 1),
		})
	};
	let after = if new_allocation.end() == &MAX_PAGE { 
		None
	} else {
		Some(Chunk {
			allocated: false,
			pages: PageRange::new(*new_allocation.end() + 1, *chosen_chunk.pages.end()),
		})
	};

	// some strict sanity checks -- these can be removed or disabled for better performance
	if let Some(ref b) = before {
		assert!(!new_allocation.contains(b.end()));
		assert!(!b.contains(new_allocation.start()));
	}
	if let Some(ref a) = after {
		assert!(!new_allocation.contains(a.start()));
		assert!(!a.contains(new_allocation.end()));
	}
	

	let deferred_action: DeferredAllocAction;
	// Since we're updating the chunk in-place here, we need to make sure we preserve the ordering of the free pages list. 
	// Thus, we set that chunk to be the `before` chunk, unless `before` is zero-sized, 
	// in which case we set that chunk to be `new_allocation`.
	match before {
		Some(b) if b.size_in_pages() > 0 => {
			chosen_chunk_ref.replace_with(b).expect("BUG: failed to replace allocator chunk");
			deferred_action = DeferredAllocAction::new(
				new_allocation.clone(),
				after,
				None,
			);
		}
		_ => {
			chosen_chunk_ref.replace_with(new_allocation.clone()).expect("BUG: failed to replace allocator chunk");
			deferred_action = DeferredAllocAction::new(
				None, // we already set this chunk in-place to the newly-allocated chunk above
				before,
				after,
			);
		}
	}

	Ok((new_allocation.into_allocated_pages(), deferred_action))
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
	num_pages: usize,
) -> Result<(AllocatedPages, DeferredAllocAction<'static>), &'static str> {
	if num_pages == 0 {
		// warn!("PageAllocator: requested an allocation of 0 pages... stupid!");
		return Err("cannot allocate zero pages");
	}

	let mut locked_list = MEM_CHUNK_TREE.lock();

	// The main logic of the allocator is to find an appropriate chunk that can satisfy the allocation request.
	// An appropriate chunk satisfies the following conditions:
	// - Can fit the requested size (starting at the requested address) within the chunk
	// - The chunk can only be within in a designated region if a specific address was requested
	// - The chunk itself is not marked as allocated
	find_any_chunk(&mut locked_list, num_pages)
	    .map_err(From::from) // convert from AllocationError to &str
}


/// Similar to [`allocated_pages_deferred()`](fn.allocate_pages_deferred.html),
/// but accepts a size value for the allocated pages in number of bytes instead of number of pages. 
/// 
/// This function still allocates whole pages by rounding up the number of bytes. 
pub fn allocate_pages_by_bytes_deferred(
	num_bytes: usize,
) -> Result<(AllocatedPages, DeferredAllocAction<'static>), &'static str> {
	let num_pages = (num_bytes + PAGE_SIZE - 1) / PAGE_SIZE; // round up
	allocate_pages_deferred(num_pages)
}


/// Allocates the given number of pages with no constraints on the starting virtual address.
/// 
/// See [`allocate_pages_deferred()`](fn.allocate_pages_deferred.html) for more details. 
pub fn allocate_pages(num_pages: usize) -> Option<AllocatedPages> {
	allocate_pages_deferred(num_pages)
		.map(|(ap, _action)| ap)
		.ok()
}


/// Allocates pages with no constraints on the starting virtual address, 
/// with a size given by the number of bytes. 
/// 
/// This function still allocates whole pages by rounding up the number of bytes. 
/// See [`allocate_pages_deferred()`](fn.allocate_pages_deferred.html) for more details. 
pub fn allocate_pages_by_bytes(num_bytes: usize) -> Option<AllocatedPages> {
	allocate_pages_by_bytes_deferred(num_bytes)
		.map(|(ap, _action)| ap)
		.ok()
}
