use alloc::boxed::Box;
use core::marker::PhantomData;
use intrusive_collections::{
    intrusive_adapter,
    rbtree::{RBTree, RBTreeOps},
    RBTreeLink,
	Adapter,
	KeyAdapter,
	PointerOps,
};

/// A wrapper for the type stored in the `StaticArrayRBTree::Inner::RBTree` variant.
struct Wrapper<T: Ord> {
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

impl <T: Ord> Wrapper<T> {
    /// Convenience method for creating a new link
    fn new_link(value: T) -> Box<Self> {
        Box::new(Wrapper {
            link: RBTreeLink::new(),
            inner: value,
        })
    }
}


/// A convenience wrapper that abstracts either an intrustive `RBTree<T>` or a primitive array `[T; N]`.
/// 
/// This allows the caller to create an array statically in a const context, 
/// and then abstract over both that and the inner `RBTree` when using it. 
/// 
/// TODO: use const generics to allow this to be of any arbitrary size beyond 32 elements.
pub struct StaticArrayRBTree<T: Ord>(Inner<T>);

/// The inner enum, hidden for visibility reasons because Rust lacks private enum variants.
enum Inner<T: Ord> {
	Array([Option<T>; 32]),
	RBTree(RBTree<WrapperAdapter<T>>),
}

impl<T: Ord> Default for StaticArrayRBTree<T> {
	fn default() -> Self {
		Self::new()
	}
}
impl<T: Ord> StaticArrayRBTree<T> {
	pub const fn new() -> Self {
		StaticArrayRBTree(Inner::Array([None; 32]))
	}
}


impl<T: Ord + 'static> StaticArrayRBTree<T> {
    /// Push the given `value` into this collection.
    ///
    /// If the inner collection is an array, it is pushed onto the back of the array.
    /// If there is no space left in the array, an `Err` containing the given `value` is returned.
    //
    // pub fn insert<'a>(&'a mut self, val: <A::PointerOps as PointerOps>::Pointer) -> CursorMut<'_, A>
    // where
    //     <A as KeyAdapter<'a>>::Key: Ord,
    // {
    // pub fn insert<'a>(&'a mut self, value: T) -> Result<(), T> where T: 'a{
	pub fn insert(&mut self, value: T) -> Result<(), T> {
		match &mut self.0 {
			Inner::Array(arr) => {
				for elem in arr {
					if elem.is_none() {
						*elem = Some(value);
						return Ok(());
					}
				}
				error!("Out of space in array, failed to insert value.");
				Err(value)
			}
			Inner::RBTree(tree) => {
                tree.insert(Wrapper::new_link(value));
				Ok(())
			}
		}
	}

	/// Converts the contained collection from a primitive array into a RBTree.
	/// If the contained collection is already using heap allocation, this is a no-op.
	/// 
	/// Call this function once heap allocation is available. 
	pub fn convert_to_heap_allocated(&mut self) {
		let new_tree = match &mut self.0 {
			Inner::Array(arr) => {
				let mut tree = RBTree::new(WrapperAdapter::new());
				for elem in arr {
					if let Some(e) = elem.take() {
						tree.insert(Wrapper::new_link(e));
					}
				}
				tree
			}
			Inner::RBTree(_tree) => return,
		};
		*self = StaticArrayRBTree(Inner::RBTree(new_tree));
	}

	/// Returns a forward iterator over references to items in this collection.
	pub fn iter(&self) -> impl Iterator<Item = &T> {
		let mut iter_a = None;
		let mut iter_b = None;
		match &self.0 {
			Inner::Array(arr)     => iter_a = Some(arr.iter().flatten()),
			Inner::RBTree(tree) => iter_b = Some(tree.iter()),
		}
        iter_a.into_iter()
            .flatten()
            .chain(
                iter_b.into_iter()
                    .flatten()
                    .map(|wrapper| &wrapper.inner)
                )
	}

	// /// Returns a forward iterator over mutable references to items in this collection.
	// pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
	// 	let mut iter_a = None;
	// 	let mut iter_b = None;
	// 	match self {
	// 		StaticArrayRBTree::Array(arr)     => iter_a = Some(arr.iter_mut().flatten()),
	// 		StaticArrayRBTree::RBTree(ll) => iter_b = Some(ll.iter_mut()),
	// 	}
	// 	iter_a.into_iter().flatten().chain(iter_b.into_iter().flatten())
	// }
}