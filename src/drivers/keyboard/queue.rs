//! A simple and easy wrapper around `Vec` to implement a FIFO queue. This is
//! no fancy, advanced data type but something simple you can use easily until
//! or unless you need something different.
//!
//! # Example
//!
//! ```
//! use queue::Queue;
//!
//! let mut q = Queue::new();
//!
//! q.queue("hello").unwrap();
//! q.queue("out").unwrap();
//! q.queue("there!").unwrap();
//!
//! while let Some(item) = q.dequeue() {
//!     println!("{}", item);
//! }
//! ```
//!
//! Outputs:
//!
//! ```text
//! hello
//! out
//! there!
//! ```

#![allow(dead_code)]
#![no_std] // KevinBoos: added this

#[cfg(test)]
mod tests;

use collections::Vec;


/// A first in, first out queue built around `Vec`.
///
/// An optional capacity can be set (or changed) to ensure the `Queue` never
/// grows past a certain size. If the capacity is not specified (ie set to
/// `None`) then the `Queue` will grow as needed. If you're worried about
/// memory allocation, set a capacity and the necessary memory will be
/// allocated at that time. Otherwise memory could be allocated, deallocated
/// and reallocated as the `Queue` changes size.
///
/// The only requirement of the type used is that it implements the `Clone`
/// trait.
///
/// # Example
///
/// ```
/// use queue::Queue;
///
/// let mut q = Queue::with_capacity(5);
///
/// for i in 0..5 {
/// 	q.queue(i).unwrap();
/// }
///
/// for i in 0..5 {
/// 	assert_eq!(q.dequeue(), Some(i));
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Queue<T> {
	vec: Vec<T>,
	cap: Option<usize>,
}

impl<T: Clone> Queue<T> {
	/// Constructs a new `Queue<T>`.
	///
	/// # Example
	///
	/// ```
	/// # use queue::Queue;
	/// let mut q: Queue<String> = Queue::new();
	/// ```
	pub fn new() -> Queue<T> {
		Queue {
			vec: Vec::new(),
			cap: None,
		}
	}

	/// Constructs a new `Queue<T>` with a specified capacity.
	///
	/// # Example
	///
	/// ```
	/// # use queue::Queue;
	/// let mut q: Queue<String> = Queue::with_capacity(20);
	/// ```
	pub fn with_capacity(cap: usize) -> Queue<T> {
		Queue {
			vec: Vec::with_capacity(cap),
			cap: Some(cap),
		}
	}

	/// Add an item to the end of the `Queue`. Returns `Ok(usize)` with the new
	/// length of the `Queue`, or `Err(())` if there is no more room.
	///
	/// # Example
	///
	/// ```
	/// # use queue::Queue;
	/// let mut q = Queue::new();
	/// q.queue("hello").unwrap();
	/// assert_eq!(q.peek(), Some("hello"));
	/// ```
	pub fn queue(&mut self, item: T) -> Result<usize, ()> {
		if let Some(cap) = self.cap {
			if self.vec.len() >= cap {
				Err(())
			} else {
				self.vec.push(item);
				Ok(self.vec.len())
			}
		} else {
			self.vec.push(item);
			Ok(self.vec.len())
		}
	}

	/// Remove the next item from the `Queue`. Returns `Option<T>` so it will
	/// return either `Some(T)` or `None` depending on if there's anything in
	/// the `Queue` to get.
	///
	/// # Example
	///
	/// ```
	/// # use queue::Queue;
	/// let mut q = Queue::new();
	/// q.queue("hello").unwrap();
	/// q.queue("world").unwrap();
	/// assert_eq!(q.dequeue(), Some("hello"));
	/// ```
	pub fn dequeue(&mut self) -> Option<T> {
		if self.vec.len() > 0 {
			Some(self.vec.remove(0))
		} else {
			None
		}
	}

	/// Peek at the next item in the `Queue`, if there's something there.
	///
	/// # Example
	///
	/// ```
	/// # use queue::Queue;
	/// let mut q = Queue::new();
	/// q.queue(12).unwrap();
	/// assert_eq!(q.peek(), Some(12));
	/// ```
	pub fn peek(&self) -> Option<T> {
		if self.vec.len() > 0 {
			Some(self.vec[0].clone())
		} else {
			None
		}
	}

	/// The number of items currently in the `Queue`.
	///
	/// # Example
	///
	/// ```
	/// # use queue::Queue;
	/// let mut q = Queue::with_capacity(8);
	/// q.queue(1).unwrap();
	/// q.queue(2).unwrap();
	/// assert_eq!(q.len(), 2);
	/// ```
	pub fn len(&self) -> usize {
		self.vec.len()
	}

	/// Query the capacity for a `Queue`. If there is no capacity set (the
	/// `Queue` can grow as needed) then `None` will be returned, otherwise
	/// it will be `Some(usize)`.
	///
	/// # Example
	///
	/// ```
	/// # use queue::Queue;
	/// let q: Queue<u8> = Queue::with_capacity(12);
	/// assert_eq!(q.capacity(), Some(12));
	/// ```
	pub fn capacity(&self) -> Option<usize> {
		self.cap
	}

	/// Modify the capacity of a `Queue`. If set to `None`, the `Queue` will
	/// grow automatically, as needed. Otherwise, it will be limited to the
	/// specified number of items. If there are more items in the `Queue` than
	/// the requested capacity, `Err(())` will be returned, otherwise the
	/// operation will succeed and `Ok(())` will be returned. If the capacity
	/// is shrunk, the underlying `Vec` will be shrunk also, which would free
	/// up whatever extra memory was allocated for the `Queue`.
	///
	/// # Example
	///
	/// ```
	/// # use queue::Queue;
	/// let mut q: Queue<u8> = Queue::new();
	/// q.set_capacity(12).unwrap();
	/// q.set_capacity(None).unwrap();
	/// ```
	pub fn set_capacity<C: Into<Option<usize>>>(&mut self, cap: C) -> Result<(), ()> {
		let cap = cap.into();

		if cap == None {
			self.cap = None;
			return Ok(());
		}

		if cap == self.cap {
			return Ok(());
		}

		let cap = cap.unwrap();

		if cap < self.vec.len() {
			return Err(());
		}

		if let Some(scap) = self.cap {
			if cap < scap {
				self.vec.shrink_to_fit();
			}
		}

		let r = cap - self.vec.len();
		self.vec.reserve_exact(r);
		self.cap = Some(cap);

		Ok(())
	}
}