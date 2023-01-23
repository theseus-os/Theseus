// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//
// Allocator code taken from Philipp Oppermann's blog "Writing an OS in Rust" (https://github.com/phil-opp/blog_os/tree/post-11)
// and modified by Ramla Ijaz


// TODO: add documentation to each unsafe block, laying out all the conditions under which it's safe or unsafe to use it.
#![allow(clippy::missing_safety_doc)]
#![feature(const_mut_refs)]
#![no_std]

extern crate alloc;
extern crate linked_list_allocator;

use alloc::alloc::Layout;
use core::{
    mem,
    ptr::{self, NonNull},
};

/// The block sizes to use.
///
/// The sizes must each be power of 2 because they are also used as
/// the block alignment (alignments must be always powers of 2).
const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

/// Choose an appropriate block size for the given layout.
///
/// Returns an index into the `BLOCK_SIZES` array.
fn list_index(layout: &Layout) -> Option<usize> {
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}

struct ListNode {
    next: Option<&'static mut ListNode>,
}

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: linked_list_allocator::Heap,
}

impl FixedSizeBlockAllocator {

    /// Creates an empty FixedSizeBlockAllocator.
    pub const fn new() -> Self {
        const SIZE: usize = BLOCK_SIZES.len();
        const INIT_VALUE: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator {
            list_heads: [INIT_VALUE; SIZE],
            fallback_allocator: linked_list_allocator::Heap::empty(),
        }
    }

    /// Initialize the allocator with the given heap bounds.
    ///
    /// This function is unsafe because the caller must guarantee that the given
    /// heap bounds are valid and that the heap is unused. This method must be
    /// called only once.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.fallback_allocator.init(heap_start, heap_size);
    }

    /// Allocates using the fallback allocator.
    fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
        match self.fallback_allocator.allocate_first_fit(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => ptr::null_mut(),
        }
    }

    /// Allocates a chunk of the given size with the given alignment. Returns a pointer to the
    /// beginning of that chunk if it was successful. Else it returns a null pointer.
    /// 
    /// Allocator first tries to find the smallest block size that is greater or equal to the required size. 
    /// If a block of that size is available then it is returned, 
    /// otherwise it tries to allocate from the fallback allocator.
    pub unsafe fn allocate(&mut self, layout: Layout) -> *mut u8 {
        match list_index(&layout) {
            Some(index) => {
                match self.list_heads[index].take() {
                    Some(node) => {
                        self.list_heads[index] = node.next.take();
                        node as *mut ListNode as *mut u8
                    }
                    None => {
                        // no block exists in list => allocate new block
                        let block_size = BLOCK_SIZES[index];
                        // only works if all block sizes are a power of 2
                        let block_align = block_size;
                        let layout = Layout::from_size_align(block_size, block_align).unwrap();
                        self.fallback_alloc(layout)
                    }
                }
            }
            None => self.fallback_alloc(layout),
        }
    } 

    /// Frees the given allocation. `ptr` must be a pointer returned
    /// by a call to the `allocate` function with identical size and alignment. Undefined
    /// behavior may occur for invalid arguments, thus this function is unsafe.
    /// 
    /// If the allocation returned is one of the fixed block sizes, 
    /// then it is returned to the head of the block list.
    /// Otherwise, it is deallocated using the fallback allocator.
    pub unsafe fn deallocate(&mut self, ptr: *mut u8, layout: Layout) {
         match list_index(&layout) {
            Some(index) => {
                let new_node = ListNode {
                    next: self.list_heads[index].take(),
                };
                // verify that block has size and alignment required for storing node
                assert!(mem::size_of::<ListNode>() <= BLOCK_SIZES[index]);
                assert!(mem::align_of::<ListNode>() <= BLOCK_SIZES[index]);
                let new_node_ptr = ptr as *mut ListNode;
                new_node_ptr.write(new_node);
                self.list_heads[index] = Some(&mut *new_node_ptr);
            }
            None => {
                let ptr = NonNull::new(ptr).unwrap();
                self.fallback_allocator.deallocate(ptr, layout);
            }
        }
    }
}