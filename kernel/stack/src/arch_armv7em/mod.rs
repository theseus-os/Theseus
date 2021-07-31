//! Provides the `Stack` type that represents a Task's stack
//! and functions for allocating new stacks.

use core::ops::{Deref, DerefMut};
use kernel_config::memory::PAGE_SIZE;
use memory::{EntryFlags, MappedPages};
use memory_structs::VirtualAddress;

/// Allocates a new stack and maps it to the active page table.
///
/// This also reserves an unmapped guard page beneath the bottom of the stack
/// in order to catch stack overflows.
///
/// Returns the newly-allocated stack and a VMA to represent its mapping.
pub fn alloc_stack(size_in_pages: usize) -> Option<Stack> {
    // Allocate enough pages for an additional guard page.
    let pages = page_allocator::allocate_pages(size_in_pages + 1)?;
    if let Some(kmmi) = memory::get_kernel_mmi_ref() {
        let dummy_page_table = &mut kmmi.lock().page_table;
        let dummy_mapped_pages = match dummy_page_table.map_allocated_pages(
            pages,
            EntryFlags::WRITABLE
        ) {
            Ok(mapped_pages) => mapped_pages,
            Err(_) => return None
        };
        return Stack::from_pages(dummy_mapped_pages).ok();
    }
    None
}

/// A range of mapped memory designated for use as a task's stack.
///
/// There is an unmapped guard page beneath the stack,
/// which is a standard approach to detect stack overflow.
///
/// A stack is backed by and auto-derefs into `MappedPages`.
#[derive(Debug)]
pub struct Stack {
    pages: MappedPages
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
        stack_pages: MappedPages
    ) -> Result<Stack, MappedPages> {
        if stack_pages.flags().is_writable() {
            Ok(Stack {
                pages: stack_pages,
            })
        } else {
            Err(stack_pages)
        }
    }
}
