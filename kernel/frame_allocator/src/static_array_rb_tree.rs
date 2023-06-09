use alloc::boxed::Box;
use core::ops::{Deref, DerefMut};
use intrusive_collections::{
    intrusive_adapter,
    rbtree::{RBTree, CursorMut},
    RBTreeLink,
	KeyAdapter,
};

/// A wrapper for the type stored in the `StaticArrayRBTree::Inner::RBTree` variant.
#[derive(Debug)]
pub struct Wrapper<T: Ord> {
    link: RBTreeLink,
    inner: T,
}
intrusive_adapter!(pub WrapperAdapter<T> = Box<Wrapper<T>>: Wrapper<T> { link: RBTreeLink } where T: Ord);

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
    pub(crate) fn new_link(value: T) -> Box<Self> {
        Box::new(Wrapper {
            link: RBTreeLink::new(),
            inner: value,
        })
    }

	pub(crate) fn into_inner(self) -> T {
		self.inner
	}
}


/// A convenience wrapper that abstracts either an intrustive `RBTree<T>` or a primitive array `[T; N]`.
/// 
/// This allows the caller to create an array statically in a const context, 
/// and then abstract over both that and the inner `RBTree` when using it. 
/// 
/// TODO: use const generics to allow this to be of any arbitrary size beyond 32 elements.
pub struct StaticArrayRBTree<T: Ord>(pub(crate) Inner<T>);

/// The inner enum, hidden for visibility reasons because Rust lacks private enum variants.
pub(crate) enum Inner<T: Ord> {
	Array([Option<T>; 32]),
	RBTree(RBTree<WrapperAdapter<T>>),
}

impl<T: Ord> Default for StaticArrayRBTree<T> {
	fn default() -> Self {
		Self::empty()
	}
}
impl<T: Ord> StaticArrayRBTree<T> {
	/// Create a new empty collection (the default).
	pub const fn empty() -> Self {
		// Yes, this is stupid... Rust seems to have removed the [None; 32] array init syntax.
		StaticArrayRBTree(Inner::Array([
			None, None, None, None, None, None, None, None,
			None, None, None, None, None, None, None, None,
			None, None, None, None, None, None, None, None,
			None, None, None, None, None, None, None, None,
		]))
	}

	/// Create a new collection based on the given array of values.
	#[allow(dead_code)]
	pub const fn new(arr: [Option<T>; 32]) -> Self {
		StaticArrayRBTree(Inner::Array(arr))
	}
}


impl<T: Ord + 'static> StaticArrayRBTree<T> {
    /// Push the given `value` into this collection.
    ///
    /// If the inner collection is an array, it is pushed onto the back of the array.
	/// If there is no space left in the array, an `Err` containing the given `value` is returned.
	///
	/// If success
	pub fn insert(&mut self, value: T) -> Result<ValueRefMut<T>, T> {
		match &mut self.0 {
			Inner::Array(arr) => {
				for elem in arr {
					if elem.is_none() {
						*elem = Some(value);
						return Ok(ValueRefMut::Array(elem));
					}
				}
				error!("Out of space in StaticArrayRBTree's inner array, failed to insert value.");
				Err(value)
			}
			Inner::RBTree(tree) => {
                Ok(ValueRefMut::RBTree(
					tree.insert(Wrapper::new_link(value))
				))
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

	/// Returns the number of elements in this collection. 
	///
	/// Note that this an O(N) linear-time operation, not an O(1) constant-time operation. 
	/// This is because the internal collection types do not separately maintain their current length.
	pub fn len(&self) -> usize {
		match &self.0 {
			Inner::Array(arr) => arr.iter().filter(|e| e.is_some()).count(),
			Inner::RBTree(tree) => tree.iter().count(),
		}
	}

	/// Returns a forward iterator over immutable references to items in this collection.
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


pub enum RemovedValue<T: Ord> {
	Array(Option<T>),
	RBTree(Option<Box<Wrapper<T>>>),
}
impl<T: Ord> RemovedValue<T> {
	#[allow(dead_code)]
	pub fn as_ref(&self) -> Option<&T> {
		match self {
			Self::Array(opt) => opt.as_ref(),
			Self::RBTree(opt) => opt.as_ref().map(|bw| &bw.inner),
		}
	}
}

/// A mutable reference to a value in the `StaticArrayRBTree`. 
pub enum ValueRefMut<'list, T: Ord> {
	Array(&'list mut Option<T>),
	RBTree(CursorMut<'list, WrapperAdapter<T>>),
}
impl <'list, T: Ord> ValueRefMut<'list, T> {
	/// Removes this value from the collection and returns the removed value, if one existed. 
	pub fn remove(&mut self) -> RemovedValue<T> {
		match self {
			Self::Array(ref mut arr_ref) => {
				RemovedValue::Array(arr_ref.take())
			}
			Self::RBTree(ref mut cursor_mut) => {
				RemovedValue::RBTree(cursor_mut.remove())
			}
		}
	}


	/// Removes this value from the collection and replaces it with the given `new_value`. 
	///
	/// Returns the removed value, if one existed. If not, the 
	#[allow(dead_code)]
	pub fn replace_with(&mut self, new_value: T) -> Result<Option<T>, T> {
		match self {
			Self::Array(ref mut arr_ref) => {
				Ok(arr_ref.replace(new_value))
			}
			Self::RBTree(ref mut cursor_mut) => {
				cursor_mut.replace_with(Wrapper::new_link(new_value))
					.map(|removed| Some(removed.inner))
					.map_err(|e| e.inner)
			}
		}
	}

	#[allow(dead_code)]
	pub fn get(&self) -> Option<&T> {
		match self {
			Self::Array(ref arr_ref) => arr_ref.as_ref(),
			Self::RBTree(ref cursor_mut) => cursor_mut.get().map(|w| w.deref()),
		}
	}
}