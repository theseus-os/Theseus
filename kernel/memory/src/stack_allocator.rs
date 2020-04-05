use super::paging::*;
use super::{PAGE_SIZE, FrameAllocator, VirtualAddress, EntryFlags, PageRange};
use super::Mapper;

#[derive(Debug)]
pub struct StackAllocator {
    pub range: PageRange,
    pub usermode: bool,
}

impl StackAllocator {
    /// Create a new `StackAllocator` that allocates random frames
    /// and maps them to the given range of `Page`s.
    pub fn new(range: PageRange, usermode: bool) -> StackAllocator {
        StackAllocator { 
            range: range, 
            usermode: usermode,
        }
    }
}

impl StackAllocator {
    
    /// Allocates a new stack and maps it to the active page table. 
    /// The given `page_table` can be a `PageTable` or a `Mapper`, 
    /// because `PageTable` automatically derefs into a `Mapper`.
    /// Reserves an unmapped guard page to catch stack overflows. 
    /// The given `usermode` argument determines whether the stack is accessible from userspace.
    /// Returns the newly-allocated stack and a VMA to represent its mapping.
    pub fn alloc_stack<FA>(&mut self, page_table: &mut Mapper, frame_allocator: &mut FA, size_in_pages: usize)
            -> Option<Stack> where FA: FrameAllocator 
    {
        warn!("alloc_stack: size_in_pages: {}", size_in_pages);
        if size_in_pages == 0 {
            return None; /* a zero sized stack maikes no sense */
        }

        // minimum required flag is WRITABLE
        let flags = if self.usermode { EntryFlags::USER_ACCESSIBLE | EntryFlags::WRITABLE} else { EntryFlags::WRITABLE };

        // clone the range, since we only want to change it on success
        let mut range = self.range.clone();

        // try to allocate the stack pages and a guard page
        let guard_page = range.next();
        let stack_start = range.next();
        let stack_end = if size_in_pages == 1 {
            stack_start
        } else {
            // choose the (size_in_pages-2)th element, since index
            // starts at 0 and we already allocated the start page
            range.nth(size_in_pages - 2)
        };

        match (guard_page, stack_start, stack_end) {
            (Some(_), Some(start), Some(end)) => {
                // success! write back updated range
                self.range = range;

                // map stack pages to physical frames
                // but don't map the guard page, that should be left unmapped
                warn!("mapping stack pages from start {:?} to end {:?} with flags {:?}", start, end, flags);
                let stack_pages = match page_table.map_pages(PageRange::new(start, end), flags, frame_allocator) {
                    Ok(pages) => pages,
                    Err(e) => {
                        error!("alloc_stack(): couldn't map_pages for the new Stack, error: {}", e);
                        return None;
                    }
                };
                warn!("succesfully mapped stack pages! {:?}", stack_pages);

                // create a new stack
                // stack grows downward from the top address (which is the last page's start_addr + page size)
                let top_of_stack = end.start_address() + PAGE_SIZE;
                Some(Stack::new(top_of_stack, start.start_address(), stack_pages))
            }
            _ => {
                error!("alloc_stack failed, not enough free pages to allocate {}!", size_in_pages);
                None /* not enough pages */
            }
        }
    }
}

#[derive(Debug)]
pub struct Stack {
    top: VirtualAddress,
    bottom: VirtualAddress,
    pages: MappedPages,
}

impl Stack {
    pub fn new(top: VirtualAddress, bottom: VirtualAddress, pages: MappedPages) -> Stack {
        assert!(top > bottom);
        Stack {
            top: top,
            bottom: bottom,
            pages: pages,
        }
    }

    /// the top of this Stack. This address is not dereferenceable, the one right below it is. 
    /// to get the highest usable address in this Stack, call `top_usable()`
    pub fn top_unusable(&self) -> VirtualAddress {
        self.top
    }

    /// Returns the highest usable address of this Stack, 
    /// which is top_unusable() - sizeof(VirtualAddress)
    pub fn top_usable(&self) -> VirtualAddress {
        use core::mem;
        self.top - mem::size_of::<VirtualAddress>()
    }


    pub fn bottom(&self) -> VirtualAddress {
        self.bottom
    }

    pub fn size(&self) -> usize {
        self.top_unusable().value() - self.bottom.value()
    }
}
