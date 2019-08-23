use core::{
    ops::{RangeInclusive, Add, AddAssign, Sub, SubAssign, Deref, DerefMut},
    mem,
    iter::Step,
};
use super::*;


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Page {
    number: usize, 
}
impl fmt::Debug for Page {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Page(vaddr: {:#X})", self.start_address()) 
    }
}

impl Page {
    /// Returns the `Page` that contains the given `VirtualAddress`.
    pub fn containing_address(virt_addr: VirtualAddress) -> Page {
        Page { number: virt_addr.value() / PAGE_SIZE }
    }

	/// Returns the `VirtualAddress` as the start of this `Page`.
    pub fn start_address(&self) -> VirtualAddress {
        // Cannot create VirtualAddress directly because the field is private
        VirtualAddress::new_canonical(self.number * PAGE_SIZE)
    }

	/// Returns the 9-bit part of this page's virtual address that is the index into the P4 page table entries list.
    pub fn p4_index(&self) -> usize {
        (self.number >> 27) & 0x1FF
    }

    /// Returns the 9-bit part of this page's virtual address that is the index into the P3 page table entries list.
    pub fn p3_index(&self) -> usize {
        (self.number >> 18) & 0x1FF
    }

    /// Returns the 9-bit part of this page's virtual address that is the index into the P2 page table entries list.
    pub fn p2_index(&self) -> usize {
        (self.number >> 9) & 0x1FF
    }

    /// Returns the 9-bit part of this page's virtual address that is the index into the P2 page table entries list.
    /// Using this returned `usize` value as an index into the P1 entries list will give you the final PTE, 
    /// from which you can extract the mapped `Frame` (or its physical address) using `pointed_frame()`.
    pub fn p1_index(&self) -> usize {
        (self.number >> 0) & 0x1FF
    }
}

impl Add<usize> for Page {
    type Output = Page;

    fn add(self, rhs: usize) -> Page {
        // cannot exceed max page number
        Page {
            number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs)),
        }
    }
}

impl AddAssign<usize> for Page {
    fn add_assign(&mut self, rhs: usize) {
        *self = Page {
            number: core::cmp::min(MAX_PAGE_NUMBER, self.number.saturating_add(rhs)),
        };
    }
}

impl Sub<usize> for Page {
    type Output = Page;

    fn sub(self, rhs: usize) -> Page {
        Page { number: self.number.saturating_sub(rhs) }
    }
}

impl SubAssign<usize> for Page {
    fn sub_assign(&mut self, rhs: usize) {
        *self = Page {
            number: self.number.saturating_sub(rhs),
        };
    }
}

// Implementing these functions allow `Page` to be in an `Iterator`.
impl Step for Page {
    #[inline]
    fn steps_between(start: &Page, end: &Page) -> Option<usize> {
        Step::steps_between(&start.number, &end.number)
    }
    #[inline]
    fn replace_one(&mut self) -> Self {
        mem::replace(self, Page { number: 1 })
    }
    #[inline]
    fn replace_zero(&mut self) -> Self {
        mem::replace(self, Page { number: 0 })
    }
    #[inline]
    fn add_one(&self) -> Self {
        Add::add(*self, 1)
    }
    #[inline]
    fn sub_one(&self) -> Self {
        Sub::sub(*self, 1)
    }
    #[inline]
    fn add_usize(&self, n: usize) -> Option<Page> {
        Some(*self + n)
    }
}



/// A range of `Page`s that are contiguous in virtual memory.
#[derive(Debug, Clone)]
pub struct PageRange(RangeInclusive<Page>);

impl PageRange {
    /// Creates a new range of `Page`s that spans from `start` to `end`,
    /// both inclusive bounds.
    pub fn new(start: Page, end: Page) -> PageRange {
        PageRange(RangeInclusive::new(start, end))
    }

    /// Creates a PageRange that will always yield `None`.
    pub fn empty() -> PageRange {
        PageRange::new(Page { number: 1 }, Page { number: 0 })
    }
    
    /// A convenience method for creating a new `PageRange` 
    /// that spans all `Page`s from the given virtual address 
    /// to an end bound based on the given size.
    pub fn from_virt_addr(starting_virt_addr: VirtualAddress, size_in_bytes: usize) -> PageRange {
        let start_page = Page::containing_address(starting_virt_addr);
        let end_page = Page::containing_address(starting_virt_addr + size_in_bytes - 1);
        PageRange::new(start_page, end_page)
    }

    /// Returns the `VirtualAddress` of the starting `Page` in this `PageRange`.
    pub fn start_address(&self) -> VirtualAddress {
        self.0.start().start_address()
    }

    /// Returns the number of `Page`s covered by this iterator. 
    /// Use this instead of the Iterator trait's `count()` method.
    /// This is instant, because it doesn't need to iterate over each entry, unlike normal iterators.
    pub fn size_in_pages(&self) -> usize {
        // add 1 because it's an inclusive range
        self.0.end().number + 1 - self.0.start().number
    }

    /// Whether this `PageRange` contains the given `VirtualAddress`.
    pub fn contains_virt_addr(&self, virt_addr: VirtualAddress) -> bool {
        self.0.contains(&Page::containing_address(virt_addr))
    }

    /// Returns the offset of the given `VirtualAddress` within this `PageRange`,
    /// i.e., the difference between `virt_addr` and `self.start()`.
    pub fn offset_from_start(&self, virt_addr: VirtualAddress) -> Option<usize> {
        if self.contains_virt_addr(virt_addr) {
            Some(virt_addr.value() - self.start_address().value())
        } else {
            None
        }
    }
}

impl Deref for PageRange {
    type Target = RangeInclusive<Page>;
    fn deref(&self) -> &RangeInclusive<Page> {
        &self.0
    }
}
impl DerefMut for PageRange {
    fn deref_mut(&mut self) -> &mut RangeInclusive<Page> {
        &mut self.0
    }
}

impl IntoIterator for PageRange {
    type Item = Page;
    type IntoIter = RangeInclusive<Page>;

    fn into_iter(self) -> Self::IntoIter {
        self.0
    }
}


// /// A root (P4) page table.
// /// 
// /// Auto-derefs into a `Mapper` for easy invocation of memory mapping functions.
// pub struct PageTable {
//     mapper: Mapper,
//     p4_table: Frame,
// }
// impl fmt::Debug for PageTable {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         write!(f, "PageTable(p4: {:#X})", self.p4_table.start_address()) 
//     }
// }

// impl Deref for PageTable {
//     type Target = Mapper;

//     fn deref(&self) -> &Mapper {
//         &self.mapper
//     }
// }

// impl DerefMut for PageTable {
//     fn deref_mut(&mut self) -> &mut Mapper {
//         &mut self.mapper
//     }
// }

// impl PageTable {
//     /// An internal function to create a new top-level PageTable 
//     /// based on the currently-active page table register (e.g., CR3). 
//     pub fn from_current() -> PageTable {
//         PageTable { 
//             mapper: Mapper::from_current(),
//             p4_table: get_current_p4(),
//         }
//     }

//     /// Initializes a brand new top-level P4 `PageTable` (previously called an `InactivePageTable`)
//     /// that is based on the given `current_active_table` and is located in the given `new_p4_frame`.
//     /// The `TemporaryPage` is used for recursive mapping, and is auto-unmapped upon return. 
//     /// 
//     /// Returns the new `PageTable` that exists in physical memory at the given `new_p4_frame`, 
//     /// and has the kernel memory region mappings copied in from the given `current_page_table`
//     /// to ensure that the system will continue running 
//     pub fn new_table(
//         current_page_table: &mut PageTable,
//         new_p4_frame: Frame,
//         mut temporary_page: TemporaryPage,
//     ) -> Result<PageTable, &'static str> {
//         {
//             let table = try!(temporary_page.map_table_frame(new_p4_frame.clone(), current_page_table));
//             table.zero();

//             table[RECURSIVE_P4_INDEX].set(new_p4_frame.clone(), EntryFlags::rw_flags());

//             // start out by copying all the kernel sections into the new table
//             table.copy_entry_from_table(current_page_table.p4(), KERNEL_TEXT_P4_INDEX);
//             table.copy_entry_from_table(current_page_table.p4(), KERNEL_HEAP_P4_INDEX);
//             table.copy_entry_from_table(current_page_table.p4(), KERNEL_STACK_P4_INDEX);
//             // TODO: FIXME: we should probably copy all of the mappings here just to be safe (except 510, the recursive P4 entry.)
//         }

//         Ok( PageTable { 
//             mapper: Mapper::with_p4_frame(new_p4_frame.clone()),
//             p4_table: new_p4_frame 
//         })
//         // temporary_page is auto unmapped here 
//     }

//     /// Temporarily maps the given other `PageTable` to the recursive entry (510th entry) 
//     /// so that the given closure `f` can set up new mappings on the new `other_table` without actually switching to it yet.
//     /// Accepts a closure `f` that is passed  a `Mapper`, such that it can set up new mappings on the other table.
//     /// Consumes the given `temporary_page` and automatically unmaps it afterwards. 
//     /// # Note
//     /// This does not perform any task switching or changing of the current page table register (e.g., cr3).
//     pub fn with<F>(&mut self,
//                    other_table: &mut PageTable,
//                    mut temporary_page: temporary_page::TemporaryPage,
//                    f: F)
//         -> Result<(), &'static str>
//         where F: FnOnce(&mut Mapper) -> Result<(), &'static str>
//     {
//         let backup = get_current_p4();
//         if self.p4_table != backup {
//             return Err("To invoke PageTable::with(), that PageTable ('self') must be currently active.");
//         }

//         // map temporary_page to current p4 table
//         let p4_table = temporary_page.map_table_frame(backup.clone(), self)?;

//         // overwrite recursive mapping
//         self.p4_mut()[RECURSIVE_P4_INDEX].set(other_table.p4_table.clone(), EntryFlags::rw_flags());         
//         tlb::flush_all();

//         // set mapper's target frame to reflect that future mappings will be mapped into the other_table
//         self.mapper.target_p4 = other_table.p4_table.clone();

//         // execute f in the new context
//         let ret = f(self);

//         // restore mapper's target frame to reflect that future mappings will be mapped using the currently-active (original) PageTable
//         self.mapper.target_p4 = self.p4_table.clone();

//         // restore recursive mapping to original p4 table
//         p4_table[RECURSIVE_P4_INDEX].set(backup, EntryFlags::rw_flags());
//         tlb::flush_all();

//         // here, temporary_page is dropped, which auto unmaps it
//         ret
//     }


//     /// Switches from the currently-active page table (this `PageTable`, i.e., `self`) to the given `new_table`.
//     /// Returns the newly-switched-to PageTable.
//     pub fn switch(&mut self, new_table: &PageTable) -> PageTable {
//         // debug!("PageTable::switch() old table: {:?}, new table: {:?}", self, new_table);

//         // perform the actual page table switch
//         #[cfg(target_arch = "x86_64")]
//         set_new_p4(memory_x86::PhysicalAddress(new_table.p4_table.start_address().value() as u64));
//         let current_table_after_switch = PageTable::from_current();
//         current_table_after_switch
//     }


//     /// Returns the physical address of this page table's top-level p4 frame
//     pub fn physical_address(&self) -> PhysicalAddress {
//         self.p4_table.start_address()
//     }
// }


// /// Returns the current top-level page table frame.
// pub fn get_current_p4() -> Frame {
//     Frame::containing_address(PhysicalAddress::new_canonical(get_p4_address().0 as usize))
// }