#![no_std]
#![feature(ptr_internals)]

#[macro_use] extern crate cfg_if;

cfg_if! {
if #[cfg(mapper_spillful)] {

extern crate memory;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate kernel_config;
#[cfg(target_arch = "x86_64")]
extern crate memory_x86_64;
extern crate memory_structs;
extern crate rbtree;

use core::ptr::Unique;
use kernel_config::memory::{PAGE_SIZE, ENTRIES_PER_PAGE_TABLE};
use memory::{Page, BROADCAST_TLB_SHOOTDOWN_FUNC, Frame, PhysicalAddress, VirtualAddress, EntryFlags};
use memory::paging::table::{Table, Level4, P4};
use irq_safety::MutexIrqSafe;
use memory_structs::{PageRange};
use memory_x86_64::tlb_flush_virt_addr;
use rbtree::RBTree;

lazy_static! {
    /// The global list of VirtualMemoryAreas 
    static ref VMAS: MutexIrqSafe<RBTree<VirtualAddress, VirtualMemoryArea>> = MutexIrqSafe::new(RBTree::new());
}


pub struct MapperSpillful {
    p4: Unique<Table<Level4>>,
}

impl MapperSpillful {
    pub fn new() -> MapperSpillful {
        MapperSpillful { p4: Unique::new(P4).unwrap() } // cannot panic, we know P4 is valid.
    }

    pub fn p4(&self) -> &Table<Level4> {
        unsafe { self.p4.as_ref() }
    }

    pub fn p4_mut(&mut self) -> &mut Table<Level4> {
        unsafe { self.p4.as_mut() }
    }

    /// translates a VirtualAddress to a PhysicalAddress
    pub fn translate(&self, virtual_address: VirtualAddress) -> Option<PhysicalAddress> {
        let offset = virtual_address.value() % PAGE_SIZE;
        // get the frame number of the page containing the given virtual address,
        // and then the corresponding physical address is that PFN*sizeof(Page) + offset
        if let Some(frame) = self.translate_page(Page::containing_address(virtual_address)){
            PhysicalAddress::new(frame.number * PAGE_SIZE + offset).ok()
        }
        else {
            None
        }
    }

    pub fn translate_page(&self, page: Page) -> Option<Frame> {
        let p3 = self.p4().next_table(page.p4_index());

        let huge_page = || {
            p3.and_then(|p3| {
                let p3_entry = &p3[page.p3_index()];
                // 1GiB page?
                if let Some(start_frame) = p3_entry.pointed_frame() {
                    if p3_entry.flags().is_huge() {
                        // address must be 1GiB aligned
                        assert!(start_frame.number % (ENTRIES_PER_PAGE_TABLE * ENTRIES_PER_PAGE_TABLE) == 0);
                        return Some(Frame {
                            number: start_frame.number + page.p2_index() * ENTRIES_PER_PAGE_TABLE + page.p1_index(),
                        });
                    }
                }
                if let Some(p2) = p3.next_table(page.p3_index()) {
                    let p2_entry = &p2[page.p2_index()];
                    // 2MiB page?
                    if let Some(start_frame) = p2_entry.pointed_frame() {
                        if p2_entry.flags().is_huge() {
                            // address must be 2MiB aligned
                            assert!(start_frame.number % ENTRIES_PER_PAGE_TABLE == 0);
                            return Some(Frame { number: start_frame.number + page.p1_index() });
                        }
                    }
                }
                None
            })
        };

        p3.and_then(|p3| p3.next_table(page.p3_index()))
            .and_then(|p2| p2.next_table(page.p2_index()))
            .and_then(|p1| p1[page.p1_index()].pointed_frame())
            .or_else(huge_page)
    }


    pub fn map(&mut self, vaddr: VirtualAddress, size: usize, flags: EntryFlags) -> Result<(), &'static str> {
        // P4, P3, and P2 entries should never set NO_EXECUTE, only the lowest-level P1 entry should. 
        let mut top_level_flags = flags.clone();
        top_level_flags.set(EntryFlags::NO_EXECUTE, false);
        // top_level_flags.set(EntryFlags::WRITABLE, true); // is the same true for the WRITABLE bit?

        {
            for page in PageRange::from_virt_addr(vaddr, size).clone() {
                let af = memory::allocate_frames(1).ok_or("MapperSpillful::map() -- out of memory trying to alloc frame")?;
                let frame = *af.start();
                let p3 = self.p4_mut().next_table_create(page.p4_index(), top_level_flags);
                let p2 = p3.next_table_create(page.p3_index(), top_level_flags);
                let p1 = p2.next_table_create(page.p2_index(), top_level_flags);

                if !p1[page.p1_index()].is_unused() {
                    error!("MapperSpillful::map() page {:#x} -> frame {:#X}, page was already in use!", page.start_address(), frame.start_address());
                    return Err("page was already mapped");
                }
                p1[page.p1_index()].set_entry(frame, flags | EntryFlags::PRESENT | EntryFlags::EXCLUSIVE);
            }
        }


        VMAS.lock().insert(vaddr, VirtualMemoryArea::new(vaddr, size, flags, ""));
        Ok(())
    }

    pub fn remap(&mut self, vaddr: VirtualAddress, new_flags: EntryFlags) -> Result<(), &'static str> {
        let vmas = VMAS.lock();

        let vma  = vmas.find_node_between(&vaddr).ok_or("couldn't find corresponding VMA")?;
        let start_addr = vma.start_address();
        let size = vma.size();    
        if !(vaddr >= start_addr && vaddr <= (start_addr + size)) {
            return Err("couldn't find corresponding VMA");
        }

        if new_flags == vma.flags() {
            trace!("remap(): new_flags were the same as existing flags, doing nothing.");
            return Ok(());
        }

        let pages = PageRange::from_virt_addr(vma.start_address(), vma.size());

        for page in pages.clone() {
            let p1 = self.p4_mut()
                .next_table_mut(page.p4_index())
                .and_then(|p3| p3.next_table_mut(page.p3_index()))
                .and_then(|p2| p2.next_table_mut(page.p2_index()))
                .ok_or("mapping code does not support huge pages")?;
            
            let frame = p1[page.p1_index()].pointed_frame().ok_or("remap(): page not mapped")?;
            p1[page.p1_index()].set_entry(frame, new_flags | EntryFlags::PRESENT);

            tlb_flush_virt_addr(page.start_address());
        }
        
        if let Some(func) = BROADCAST_TLB_SHOOTDOWN_FUNC.get() {
            func(pages);
        }

        vma.set_flags(new_flags);
        Ok(())
    }

    /// Remove the virtual memory mapping for the given virtual address.
    pub fn unmap(&mut self, vaddr: VirtualAddress) -> Result<(), &'static str> {
        let mut vmas = VMAS.lock();
        let vma  = vmas.find_node_between(&vaddr).ok_or("couldn't find corresponding VMA")?;
        let start_addr = vma.start_address();
        let size = vma.size();
        if !(vaddr >= start_addr && vaddr <= (start_addr + size)) {
            return Err("couldn't find corresponding VMA");
        }

        let pages = PageRange::from_virt_addr(vma.start_address(), vma.size());

        for page in pages.clone() {
            let p1 = self.p4_mut()
                .next_table_mut(page.p4_index())
                .and_then(|p3| p3.next_table_mut(page.p3_index()))
                .and_then(|p2| p2.next_table_mut(page.p2_index()))
                .ok_or("mapping code does not support huge pages")?;
            
            let _frame = p1[page.p1_index()].pointed_frame().ok_or("unmap(): page not mapped")?;
            let _ = p1[page.p1_index()].set_unmapped();

            tlb_flush_virt_addr(page.start_address());
        }
        
        if let Some(func) = BROADCAST_TLB_SHOOTDOWN_FUNC.get() {
            func(pages);
        }

        vmas.remove(&start_addr).ok_or("Could not find VMA in VMA Tree")?;

        Ok(())
    }
}



/// A region of virtual memory that is mapped into a [`Task`](../task/struct.Task.html)'s address space
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VirtualMemoryArea {
    start: VirtualAddress,
    size: usize,
    flags: EntryFlags,
    desc: &'static str,
}
use core::fmt;
impl fmt::Display for VirtualMemoryArea {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "start: {:#X}, size: {:#X}, flags: {:#X}, desc: {}",
            self.start, self.size, self.flags, self.desc
        )
    }
}


impl VirtualMemoryArea {
    pub fn new(start: VirtualAddress, size: usize, flags: EntryFlags, desc: &'static str) -> Self {
        VirtualMemoryArea {
            start: start,
            size: size,
            flags: flags,
            desc: desc,
        }
    }

    pub fn start_address(&self) -> VirtualAddress {
        self.start
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn flags(&self) -> EntryFlags {
        self.flags
    }

    pub fn desc(&self) -> &'static str {
        self.desc
    }

    /// Get an iterator that covers all the pages in this VirtualMemoryArea
    pub fn pages(&self) -> PageRange {
        // check that the end_page won't be invalid
        if (self.start.value() + self.size) < 1 {
            return PageRange::empty();
        }

        let start_page = Page::containing_address(self.start);
        let end_page = Page::containing_address(self.start + self.size - 1);
        PageRange::new(start_page, end_page)
    }

    pub fn set_flags(&mut self, flags: EntryFlags) {
        self.flags = flags;
    }
}


}
}