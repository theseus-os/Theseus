//! Provides the `Stack` type that represents a Task's stack 
//! and functions for allocating new stacks. 

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate kernel_config;
extern crate memory_structs;
extern crate memory;
extern crate page_allocator;

use core::ops::{Deref, DerefMut};
use kernel_config::memory::PAGE_SIZE;
use memory_structs::VirtualAddress;
use memory::{EntryFlags, MappedPages, Mapper};
use page_allocator::AllocatedPages;


/// Allocates a new stack and maps it to the active page table. 
///
/// This also reserves an unmapped guard page beneath the bottom of the stack
/// in order to catch stack overflows. 
///
/// Returns the newly-allocated stack and a VMA to represent its mapping.
pub fn alloc_stack(
    size_in_pages: usize,
    page_table: &mut Mapper, 
) -> Option<Stack> {
    // Allocate enough pages for an additional guard page. 
    let pages = page_allocator::allocate_pages(size_in_pages + 1)?;
    inner_alloc_stack(pages, page_table)
}

/// The inner implementation of stack allocation. 
/// 
/// `pages` is the combined `AllocatedPages` object that holds
/// the guard page followed by the actual stack pages to be mapped.
fn inner_alloc_stack(
    pages: AllocatedPages,
    page_table: &mut Mapper, 
) -> Option<Stack> {
    let start_of_stack_pages = *pages.start() + 1; 
    let (guard_page, stack_pages) = pages.split(start_of_stack_pages).ok()?;

    // For stack memory, the minimum required flag is WRITABLE.
    let flags = EntryFlags::WRITABLE; 
    // if usermode { flags |= EntryFlags::USER_ACCESSIBLE; }

    // Map stack pages to physical frames, leave the guard page unmapped.
    let pages = match page_table.map_allocated_pages(
        stack_pages, 
        flags, 
    ) {
        Ok(pages) => pages,
        Err(e) => {
            error!("alloc_stack(): couldn't map pages for the new Stack, error: {}", e);
            return None;
        }
    };

    Some(Stack { guard_page, pages })  
}


/// A range of mapped memory designated for use as a task's stack.
/// 
/// There is an unmapped guard page beneath the stack,
/// which is a standard approach to detect stack overflow.
/// 
/// A stack is backed by and auto-derefs into `MappedPages`. 
#[derive(Debug)]
pub struct Stack {
    guard_page: AllocatedPages,
    pages: MappedPages,
}
impl Deref for Stack {
    type Target = MappedPages;
    fn deref(&self) -> &MappedPages {
        &self.pages
    }
}
impl DerefMut for Stack {
    fn deref_mut(&mut self) -> &mut MappedPages {
        &mut self.pages
    }
}

impl Stack {
    /// Returns the address just beyond the top of this stack, 
    /// which is necessary for some hardware registers to use. 
    /// 
    /// This address is not dereferenceable, the one right below it is. 
    /// To get the highest usable address in this Stack, call `top_usable()`
    pub fn top_unusable(&self) -> VirtualAddress {
        self.pages.end().start_address() + PAGE_SIZE
    }

    /// Returns the highest usable address of this Stack, 
    /// which is `top_unusable() - sizeof(VirtualAddress)`
    pub fn top_usable(&self) -> VirtualAddress {
        self.top_unusable() - core::mem::size_of::<VirtualAddress>()
    }

    /// Returns the bottom of this stack, its lowest usable address.
    pub fn bottom(&self) -> VirtualAddress {
        self.pages.start_address()
    }

    /// Creates a stack from its constituent parts: 
    /// a guard page and a series of mapped pages. 
    /// 
    /// # Conditions
    /// * The `guard_page` must be at least one page (which is unmapped) 
    ///   and must contiguously precede the `stack_pages`. 
    ///   In other words, the beginning of `stack_pages` must come 
    ///   right after the end of `guard_page`.
    /// * The `stack_pages` must be mapped as writable. 
    /// 
    /// If the conditions are not met, 
    /// an `Err` containing the given `guard_page` and `stack_pages` is returned.
    pub fn from_pages(
        guard_page: AllocatedPages,
        stack_pages: MappedPages
    ) -> Result<Stack, (AllocatedPages, MappedPages)> {
        if (*guard_page.end() + 1) == *stack_pages.start() 
            && stack_pages.flags().is_writable()
        {
            Ok(Stack { guard_page, pages: stack_pages })
        } else {
            Err((guard_page, stack_pages))
        }
    }

    /// Returns the guard page(s) for this stack. 
    ///
    /// Guard pages are virtual pages that are reserved/owned by this stack
    /// but are not mapped, causing any access to them to result in a page fault. 
    pub fn guard_page(&self) -> &memory_structs::PageRange {
        &self.guard_page
    }
}
