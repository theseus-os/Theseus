/// A generic map based on a singly-linked list data structure that is lock-free 
/// and uses `AtomicPtr`s to ensure safety in the face of multithreaded access.
/// Each node remains in the same spot in memory once it is allocated,
/// and will not be reallocated,
/// which allows an external thread to maintain a reference to it safely.
///
/// Currently we do not allow nodes to be deleted, so it's only useful for certain purposes.
/// Later on, once deletions are supported, it will not be safe to maintain out-of-band references
/// to items in the data structure, rather only weak references. 
///
/// New elements are inserted at the head of the list, and then the head's next pointer 
/// is set up to the point to the node that was previously the head. 
/// Thus, the head always points to the most recently added node. 


use alloc::boxed::Box;
use core::sync::atomic::{AtomicPtr, Ordering};

#[derive(Debug)]
struct Node<K, V> where K: PartialEq {
    key: K,
	value: V,
    next: AtomicPtr<Node<K, V>>,
}
impl<K, V> Node<K, V> where K: PartialEq {
    fn new(k: K, v: V) -> Node<K, V> {
        Node { 
            key: k,
			value: v,
            next: AtomicPtr::default(), // null ptr by default
        }
    }
}

#[derive(Debug)]
pub struct AtomicMap<K, V> where K: PartialEq {
    head: AtomicPtr<Node<K, V>>,
}

impl<K, V> AtomicMap<K, V> where K: PartialEq {
    /// Create a new empty AtomicMap.
    /// 
    /// Does not perform any allocation until a new node is created.
    pub const fn new() -> AtomicMap<K, V> {
        AtomicMap {
            head: AtomicPtr::new(core::ptr::null_mut()), // null ptr
        }
    }

    /// Adds a new key-value pair to the map. 
	/// If the given key is already present, its corresponding value will be overwritten.
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        let res = self.insert_timeout(key, value, u64::max_value());
        res.unwrap_or(None) // return the Option<V> in Ok(), or None if Err()
    }

    
	/// Adds a new key-value pair to the map. 
	/// If the given key is already present, its corresponding value will be overwritten.
	/// If it fails to do so atomically after the given number of attempts, it will abort and return Err.
    pub fn insert_timeout(&self, key: K, value: V, max_attempts: u64) -> Result<Option<V>, V> {

		// first, we check to see if the key exists in the list already.
		// if it does, simply update the corresponding value.
		for pair in self.iter_mut() {
			if key == *pair.0 {
                let old_val = ::core::mem::replace(&mut *pair.1, value);
				return Ok(Some(old_val));
			}
		}
        
		// Here, the key did not exist, so we must add a new node to hold that key-value pair.
        let node_ptr = Box::into_raw(Box::new(Node::new(key, value)));
        let max_attempts = core::cmp::max(max_attempts, 1); // ensure we try at least once 

        // start the first attempt by obtaining the current head pointer
        let mut orig_head_ptr = self.head.load(Ordering::Acquire);
        for _attempt in 0..max_attempts {

            // the new "node" will become the new head, so set the node's `next` pointer to `orig_head_ptr`
            // SAFE: we know the node_ptr is valid since we just created it above.
            unsafe {
                (*node_ptr).next = AtomicPtr::new(orig_head_ptr);
            }

            // now try to atomically swap the new `node_ptr` into the current `head` ptr
            match self.head.compare_exchange_weak(orig_head_ptr, node_ptr, Ordering::AcqRel, Ordering::Acquire) {
                // If compare_exchange succeeds, then the `head` ptr was properly updated, i.e.,
                // no other thread was interleaved and snuck in to change `head` since we last loaded it.
                Ok(_old_head_ptr) => return Ok(None),
                Err(changed_head_ptr) => orig_head_ptr = changed_head_ptr,
            }
            
            // Here, it didn't work, the head value wasn't updated, meaning that another process updated it before we could
            // so we need to start over by reading the head ptr again and trying to swap it in again
            #[cfg(test)] 
            println!("        attempt {}", _attempt);
        }

        // Here, we exceeded the number of max attempts, so we failed. 
        // Reclaim the Boxed `Node`, drop the Box, and return the inner data of type `V`.
        // SAFE: no one has touched this node except for us when we created it above.
        let reclaimed_node = unsafe {
            Box::from_raw(node_ptr)
        };

        Err(reclaimed_node.value)
    }

	/// Returns a reference to the value matching the given key, if present. 
	/// Otherwise, returns None. 
	pub fn get(&self, key: &K) -> Option<&V> {
		for pair in self.iter() {
			if key == pair.0 {
				return Some(pair.1);
			}
		}
		None
	}


    /// Returns a mutable reference to the value matching the given key, if present.
    /// Otherwise, returns None.
    /// In order to maintain memory safety (to ensure atomicity), getting a value as mutable
    /// requires `self` (this `AtomicMap` instance) to be borrowed mutably. 
    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        for pair in self.iter_mut() {
                if key == *pair.0 {
                        return Some(pair.1);
                }
        }
        None
    }


    /// Returns a forward iterator through this map. 
    pub fn iter(&self) -> AtomicMapIter<K, V> {
        AtomicMapIter {
            curr: &self.head, //load(Ordering::Acquire),
            // _phantom: PhantomData,
        }
    }

    /// This should only be used internally, as we don't want outside entities
    /// holding mutable references to data here.
    /// Returns a forward iterator through this map,
    /// allowing mutation of inner values but not keys.
    /// This is safe because we do not permit deletion from this map type.
    fn iter_mut(&self) -> AtomicMapIterMut<K, V> {
        AtomicMapIterMut {
            curr: &self.head, //load(Ordering::Acquire),
            // _phantom: PhantomData,
        }
    }
}

impl<K, V> Drop for AtomicMap<K, V> where K: PartialEq {
    fn drop(&mut self) {
        let mut curr_ptr = self.head.load(Ordering::Acquire);
        while !curr_ptr.is_null() {
            // SAFE: checked for null above
            let next_ptr = unsafe {&*curr_ptr}.next.load(Ordering::Acquire);
            let _ = unsafe { Box::from_raw(curr_ptr) }; // drop the actual Node
            curr_ptr = next_ptr;
        }
    }
}




pub struct AtomicMapIter<'a, K: PartialEq + 'a, V: 'a> {
    curr: &'a AtomicPtr<Node<K, V>>,
    // _phantom: PhantomData<&'a K, V>, // we don't need this with the &'a above
}
impl<'a, K: PartialEq + 'a, V: 'a> Iterator for AtomicMapIter<'a, K, V> {
    type Item = (&'a K, &'a V); 

    fn next(&mut self) -> Option<(&'a K, &'a V)> {
        let curr_ptr = self.curr.load(Ordering::Acquire);
        if curr_ptr == (0 as *mut _) {
            return None;
        }
        // SAFE: curr_ptr was checked for null
        let curr_node: &Node<K, V> = unsafe { &*curr_ptr };
        self.curr = &curr_node.next; // advance the iterator
        Some((&curr_node.key, &curr_node.value))
    }
}



pub struct AtomicMapIterMut<'a, K: PartialEq + 'a, V: 'a> {
    curr: &'a AtomicPtr<Node<K, V>>,
    // _phantom: PhantomData<&'a K, V>, // we don't need this with the &'a above
}
impl<'a, K: PartialEq + 'a, V: 'a> Iterator for AtomicMapIterMut<'a, K, V> {
    type Item = (&'a K, &'a mut V); 

    fn next(&mut self) -> Option<(&'a K, &'a mut V)> {
        let curr_ptr = self.curr.load(Ordering::Acquire);
        if curr_ptr == (0 as *mut _) {
            return None;
        }
        // SAFE: curr_ptr was checked for null
        let curr_node: &mut Node<K, V> = unsafe { &mut *curr_ptr };
        self.curr = &curr_node.next; // advance the iterator
        Some((&curr_node.key, &mut curr_node.value))
    }
}


#[test]
/// To run this test, execute: `cargo test test_map -- --nocapture`
fn test_map() {
    let map: AtomicMap<&'static str, u64> = AtomicMap::new();
    
	let should_be_none = map.get(&"yo");
	println!("should_be_none = {:?}", should_be_none);

	map.insert("yo", 2); 

	println!("after yo 2");
    for i in map.iter() {
        println!("    {:?}", i);
    }

	map.insert("hi", 45);
	let old_yo = map.insert("yo", 1234); 
    println!("old_yo = {:?}", old_yo);

	println!("after yo 4");
    for i in map.iter() {
        println!("    {:?}", i);
    }

	let should_be_45 = map.get(&"hi");
	println!("should_be_45 = {:?}", should_be_45);

    let should_now_be_1234 = map.get(&"yo");
    println!("should_now_be_1234 = {:?}", should_now_be_1234);
    
}
