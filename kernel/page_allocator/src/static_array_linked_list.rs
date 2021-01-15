use alloc::collections::LinkedList;

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
