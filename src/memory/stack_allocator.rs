use memory::paging::*;
use memory::{PAGE_SIZE, FrameAllocator, VirtualMemoryArea};
use memory::Mapper;
use core::ops::DerefMut;

pub struct StackAllocator {
    pub range: PageIter,
}

impl StackAllocator {
    /// Create a new `StackAllocator` that allocates random frames
    /// and maps them to the given range of `Page`s.
    pub fn new(page_range: PageIter) -> StackAllocator {
        StackAllocator { range: page_range }
    }
}

impl StackAllocator {
    
    /// Allocates a new stack and maps it to the active page table. 
    /// The given `active_table` can be an `ActivePageTable` or a `Mapper`, 
    /// because `ActivePageTable` automatically derefs into a `Mapper`.
    /// Reserves an unmapped guard page to catch stack overflows. 
    /// Returns the newly-allocated stack and a VMA to represent its mapping.
    pub fn alloc_stack<FA>(&mut self, 
                           active_table: &mut Mapper,
                           frame_allocator: &mut FA,
                           size_in_pages: usize, 
                           flags: EntryFlags)
                           -> Option<(Stack, VirtualMemoryArea)> 
                           where FA: FrameAllocator {
        if size_in_pages == 0 {
            return None; /* a zero sized stack maikes no sense */
        }

        // minimum required flag is WRITABLE
        let flags = flags | WRITABLE; // shadows the flags arg

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
                for page in Page::range_inclusive(start, end) {
                    active_table.map(page, flags, frame_allocator);
                }

                let stack_vma = VirtualMemoryArea::new(
                    start.start_address(),
                    end.start_address() - start.start_address() + PAGE_SIZE, // + 1 Page because it's an inclusive range
                    flags, 
                    if flags.contains(USER_ACCESSIBLE) { "User Stack" } else { "Kernel Stack" }, 
                );

                // create a new stack
                // stack grows downward from the top address (which is the last page's start_addr + page size)
                let top_of_stack = end.start_address() + PAGE_SIZE;
                Some( (Stack::new(top_of_stack, start.start_address()), stack_vma) )
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
    top: usize,
    bottom: usize,
}

impl Stack {
    fn new(top: usize, bottom: usize) -> Stack {
        assert!(top > bottom);
        Stack {
            top: top,
            bottom: bottom,
        }
    }

    pub fn top(&self) -> usize {
        self.top
    }

    #[allow(dead_code)]
    pub fn bottom(&self) -> usize {
        self.bottom
    }
}
