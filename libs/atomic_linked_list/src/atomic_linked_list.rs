//! A generic, singly-linked list data structure that is lock-free 
//! and uses `AtomicPtr`s to ensure safety in the face of multithreaded access.
//! Each node remains in the same spot in memory once it is allocated,
//! and will not be reallocated,
//! which allows an external thread to maintain a reference to it safely 
//! (but really, only a Weak reference is safe to maintain, to catch possible Node deletion).
//!
//! New elements are inserted at the head of the list, and then the head's next pointer 
//! is set up to the point to the node that was previously the head. 
//! Thus, the head always points to the most recently added node. 


use alloc::boxed::Box;
use core::sync::atomic::{AtomicPtr, Ordering};
// use core::marker::PhantomData;


struct Node<T> {
    data: T,
    next: AtomicPtr<Node<T>>,
}
impl<T> Node<T> {
    fn new(data: T) -> Node<T> {
        Node { 
            data: data,
            next: AtomicPtr::default(), // null ptr by default
        }
    }
}

#[derive(Debug)]
pub struct AtomicLinkedList<T> {
    head: AtomicPtr<Node<T>>,
}

impl<T> AtomicLinkedList<T> {
    /// create a new empty AtomicLinkedList.
    pub fn new() -> AtomicLinkedList<T> {
        AtomicLinkedList {
            head: AtomicPtr::default(), // null ptr
        }
    }

    /// add a new element to the front of the list.
    pub fn push_front(&self, data: T) {
        let _ = self.push_front_timeout(data, u64::max_value());
    }

    /// add a new element to the front of the list, but will abort
    /// if it fails to do so atomically after the given number of attempts. 
    pub fn push_front_timeout(&self, data: T, max_attempts: u64) -> Result<(), ()> {

        let max_attempts = if max_attempts == 0 { 1 } else { max_attempts }; // ensure we try at least once 

        let node_ptr = Box::into_raw(Box::new(Node::new(data))); // we must wrap Nodes in Box to keep them around

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
                return Ok(());
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


    /// returns a forward iterator through this linked list. 
    pub fn iter(&self) -> AtomicLinkedListIter<T> {
        AtomicLinkedListIter {
            curr: &self.head, //load(Ordering::Acquire),
            // _phantom: PhantomData,
        }
    }

    /// returns a forward iterator through this linked list,
    /// allowing mutation of inner elements.
    pub fn iter_mut(&self) -> AtomicLinkedListIterMut<T> {
        AtomicLinkedListIterMut {
            curr: &self.head, //load(Ordering::Acquire),
            // _phantom: PhantomData,
        }
    }
}


pub struct AtomicLinkedListIter<'a, T: 'a> {
    curr: &'a AtomicPtr<Node<T>>,
    // _phantom: PhantomData<&'a T>, // we don't need this with the &'a above
}
impl<'a, T: 'a> Iterator for AtomicLinkedListIter<'a, T> {
    type Item = &'a T; 

    fn next(&mut self) -> Option<&'a T> {
        let curr_ptr = self.curr.load(Ordering::Acquire);
        if curr_ptr == (0 as *mut _) {
            return None;
        }
        // SAFE: curr_ptr was checked for null
        let curr_node: &Node<T> = unsafe { &*curr_ptr };
        self.curr = &curr_node.next; // advance the iterator
        Some(&curr_node.data)
    }
}



pub struct AtomicLinkedListIterMut<'a, T: 'a> {
    curr: &'a AtomicPtr<Node<T>>,
    // _phantom: PhantomData<&'a T>, // we don't need this with the &'a above
}
impl<'a, T: 'a> Iterator for AtomicLinkedListIterMut<'a, T> {
    type Item = &'a mut T; 

    fn next(&mut self) -> Option<&'a mut T> {
        let curr_ptr = self.curr.load(Ordering::Acquire);
        if curr_ptr == (0 as *mut _) {
            return None;
        }
        // SAFE: curr_ptr was checked for null
        let curr_node: &mut Node<T> = unsafe { &mut *curr_ptr };
        self.curr = &curr_node.next; // advance the iterator
        Some(&mut curr_node.data)
    }
}


#[test]
/// To run this test, execute: `cargo test test_ll -- --nocapture`
fn test_ll() {
    use alloc::sync::Arc;
    use std::thread;

    let list: Arc<AtomicLinkedList<u64>> = Arc::new(AtomicLinkedList::new());
    
    let nthreads = 8;
    let top_range = 100;
    let mut threads = vec![];

    for id in 0..nthreads {
        let l = list.clone();
        threads.push(thread::spawn( move || {
            let start = id * top_range;
            let end = (id + 1) * top_range;
            for i in start..end {
                l.push_front(i);
            }
        }));
    }

    for t in threads {
        t.join().unwrap();
    }
    
    
    list.push_front(1);
    list.push_front(2);
    list.push_front(3);
    list.push_front(4);
    list.push_front(5);


    println!("list: {:?}", list);

    for i in list.iter() {
        println!("{:?}", i);
    }
    
}