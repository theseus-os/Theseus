//! Provides a (currently mediocre) virtual address allocator,
//! which allocates pages (not physical memory) starting from kernel_config::memory::KERNEL_TEXT_START.
//! The minimum unit of allocation is a single page.

use super::{Page, PageRange, VirtualAddress};
use alloc::collections::LinkedList;
use kernel_config::memory::{KERNEL_TEXT_MAX_SIZE, KERNEL_TEXT_START, PAGE_SIZE};
use spin::Mutex;

/// A group of contiguous pages, much like a hole in other allocators.
struct Chunk {
    /// Whether or not this Chunk is currently allocated. If false, it is free.
    allocated: bool,
    /// The Page at which this chunk starts.
    start_page: Page,
    /// The size of this chunk, specified in number of pages, not bytes.
    size_in_pages: usize,
}
impl Chunk {
    fn as_allocated_pages(&self) -> AllocatedPages {
        // subtract one because it's an inclusive range
        let end_page = self.start_page + self.size_in_pages - 1;
        AllocatedPages {
            pages: PageRange::new(self.start_page, end_page),
        }
    }
}

/// Represents an allocated range of virtual addresses, specified in pages.
/// These pages are not initially mapped to any physical memory frames, you must do that separately.
/// This object represents ownership of those pages; if this object falls out of scope,
/// it will be dropped, and the pages will be de-allocated.
/// See `MappedPages` struct for a similar object that unmaps pages when dropped.
#[derive(Debug)]
pub struct AllocatedPages {
    pub pages: PageRange,
}

impl AllocatedPages {
    /// Returns the start address of the first page.
    pub fn start_address(&self) -> VirtualAddress {
        self.pages.start_address()
    }

    pub fn size_in_pages(&self) -> usize {
        self.pages.size_in_pages()
    }
}
// use core::ops::Deref;
// impl Deref for AllocatedPages {
//     type Target = PageRange;

//     fn deref(&self) -> &PageRange {
//         &self.pages
//     }
// }

// impl Drop for AllocatedPages {
//     fn drop(&mut self) {
//         if let Err(_) = deallocate_pages(self) {
// 			error!("AllocatedPages::drop(): error deallocating pages");
// 		}
//     }
// }

lazy_static! {
    static ref FREE_PAGE_LIST: Mutex<LinkedList<Chunk>> = {
        // we need to create the first chunk here,
        // which is one giant chunk that starts at KERNEL_TEXT_START
        // and goes until the end of the kernel free text section
        let initial_chunk: Chunk = Chunk {
            allocated: false,
            start_page: Page::containing_address(VirtualAddress::new_canonical(KERNEL_TEXT_START)),
            size_in_pages: KERNEL_TEXT_MAX_SIZE / PAGE_SIZE,
        };
        let mut list: LinkedList<Chunk> = LinkedList::new();
        list.push_front(initial_chunk);
        Mutex::new(list)
    };
}

/// Convenience function for allocating pages by giving the number of bytes
/// rather than the number of pages. It will still allocate whole pages
/// by rounding up the number of bytes.
/// See [`allocate_pages()`](fn.allocate_pages.html)
pub fn allocate_pages_by_bytes(num_bytes: usize) -> Option<AllocatedPages> {
    let num_pages = (num_bytes + PAGE_SIZE - 1) / PAGE_SIZE; // round up
    allocate_pages(num_pages)
}

/// Allocates the given number of pages, but simply reserves the virtual addresses;
/// it does not allocate actual physical memory frames nor do any mapping.
/// Thus these pages aren't directly usable until they are mapped to physical frames.
/// Allocation is quick, technically O(n) but generally will allocate immediately
/// because the largest free chunks are stored at the front of the list.
/// Fragmentation isn't cleaned up until we're out of address space, but not really a big deal.
pub fn allocate_pages(num_pages: usize) -> Option<AllocatedPages> {
    if num_pages == 0 {
        warn!("allocate_pages(): requested an allocation of 0 pages... stupid!");
        return None;
    }

    // the Page where the newly-allocated Chunk starts, which we'll return if successfully allocated.
    let mut new_start_page: Option<Page> = None;

    let mut locked_list = FREE_PAGE_LIST.lock();
    for mut c in locked_list.iter_mut() {
        // skip already-allocated chunks and chunks that are too small
        if c.allocated || c.size_in_pages < num_pages {
            continue;
        }

        // here: we have found a suitable chunk
        let start_page = c.start_page;
        let remaining_size = c.size_in_pages - num_pages;
        if remaining_size == 0 {
            // if the chunk is exactly the right size, just update it in-place as 'allocated'
            c.allocated = true;
            return Some(c.as_allocated_pages());
        }

        // here: we have the chunk and we need to split it up into two chunks
        assert!(
            c.allocated == false,
            "BUG: an already-allocated chunk is going to be split!"
        );

        // first, update in-place the original free (unallocated) chunk to be smaller, since we're removing pages from it
        c.size_in_pages = remaining_size;
        c.start_page += num_pages;

        // second, create a new chunk that has the pages we've peeled off the original chunk being split
        // (or rather, we create the chunk below outside of the iterator loop, so here we just tell it where to start)
        new_start_page = Some(start_page);
        break;
    }

    if let Some(p) = new_start_page {
        let new_chunk = Chunk {
            allocated: true,
            start_page: p,
            size_in_pages: num_pages,
        };
        let ret = new_chunk.as_allocated_pages();
        locked_list.push_back(new_chunk);
        Some(ret)
    } else {
        error!("VirtualAddressAllocator: out of virtual address space.");
        return None;
    }
}

#[allow(dead_code)]
fn deallocate_pages(_pages: &mut AllocatedPages) -> Result<(), ()> {
    trace!("Virtual Address Allocator: deallocate_pages is not yet implemented, trying to dealloc: {:?}", _pages);
    Ok(())
    // unimplemented!();
}
