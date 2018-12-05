//! A generic map based on a singly-linked list data structure that is lock-free 
//! and uses `AtomicPtr`s to ensure safety in the face of multithreaded access.
//! Each node remains in the same spot in memory once it is allocated,
//! and will not be reallocated,
//! which allows an external thread to maintain a reference to it safely.
//!
//! Currently we do not allow nodes to be deleted, so it's only useful for certain purposes.
//! Later on, once deletions are supported, it will not be safe to maintain out-of-band references
//! to items in the data structure, rather only weak references. 
//!
//! New elements are inserted at the head of the list, and then the head's next pointer 
//! is set up to the point to the node that was previously the head. 
//! Thus, the head always points to the most recently added node. 


use alloc::boxed::Box;
use core::sync::atomic::{AtomicPtr, Ordering};


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
    /// create a new empty AtomicMap.
    pub fn new() -> AtomicMap<K, V> {
        AtomicMap {
            head: AtomicPtr::default(), // null ptr
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
    pub fn insert_timeout(&self, key: K, value: V, max_attempts: u64) -> Result<Option<V>, ()> {

		// first, we check to see if the key exists in the list already.
		// if it does, simply update the corresponding value.
		for pair in self.iter_mut() {
			if key == *pair.0 {
                let old_val = ::core::mem::replace(&mut *pair.1, value);
				return Ok(Some(old_val));
			}
		}


		// here, if the key did not exist, add a new node including that key-value pair
        let node_ptr = Box::into_raw(Box::new(Node::new(key, value))); // we must wrap Nodes in Box to keep them around

        let max_attempts = if max_attempts == 0 { 1 } else { max_attempts }; // ensure we try at least once 
        for _attempt in 0..max_attempts {
            // start the attempt by grabbing the head value
            let orig_head_ptr = self.head.load(Ordering::Acquire);

            // the new "node" will become the new head, so set the node's next pointer to orig_head_ptr
            // SAFE: we know the node_ptr is valid since we just created it above
            unsafe {
                (*node_ptr).next = AtomicPtr::new(orig_head_ptr);
            }

            // now try to atomically swap the new node ptr into the current head ptr
            let prev_stored_ptr = self.head.compare_and_swap(orig_head_ptr, node_ptr, Ordering::AcqRel); 
            
            // if compare_and_swap returns the same value we orig_head_ptr, then it was properly updated
            // we do this so we can check if another process 
            if prev_stored_ptr == orig_head_ptr {
                // it worked! i.e., no other process snuck in and changed head while we were setting up node.next
                return Ok(None);
            }
            else {
                // it didn't work, the head value wasn't updated, meaning that another process updated it before we could
                // so we need to start over by reading the head ptr again and trying to swap it in again
                #[test]
                println!("        attempt {}", _attempt);
            }
        }

        // clean up the unboxed node and allow it to be dropped
        // SAFE: no one has touched this node except for us when we created it above
        unsafe {
            let _ = Box::from_raw(node_ptr);
        }

        Err(())
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
    use alloc::sync::Arc;
    use std::thread;

    let list: AtomicMap<&'static str, u64> = AtomicMap::new();
    
	let should_be_none = list.get("yo");
	println!("should_be_none = {:?}", should_be_none);

	list.insert("yo", 2); 

	println!("after yo 2");
    for i in list.iter() {
        println!("    {:?}", i);
    }

	list.insert("hi", 45);
	let old_yo = list.insert("yo", 1234); 
    println!("old_yo = {:?}", old_yo);

	println!("after yo 4");
    for i in list.iter() {
        println!("    {:?}", i);
    }

	let should_be_45 = list.get("hi");
	println!("should_be_45 = {:?}", should_be_45);

    let should_now_be_1234 = list.get("yo");
    println!("should_now_be_1234 = {:?}", should_now_be_1234);
    
}
