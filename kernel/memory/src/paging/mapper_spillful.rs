use core::ptr::Unique;
use kernel_config::memory::{PAGE_SIZE, ENTRIES_PER_PAGE_TABLE, TEMPORARY_PAGE_VIRT_ADDR};
use super::super::{Page, BROADCAST_TLB_SHOOTDOWN_FUNC, VirtualMemoryArea, FrameAllocator, Frame, PhysicalAddress, VirtualAddress, EntryFlags};
use super::table::{self, Table, Level4};
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use x86_64;
#[cfg(any(target_arch = "aarch64"))]
use aarch64;



lazy_static! {
    /// The global list of VirtualMemoryAreas 
    static ref VMAS: MutexIrqSafe<Vec<VirtualMemoryArea>> = MutexIrqSafe::new(Vec::new());
}

const TEMPORARY_PAGE_FRAME: usize = TEMPORARY_PAGE_VIRT_ADDR & !(PAGE_SIZE - 1);



pub struct MapperSpillful {
    p4: Unique<Table<Level4>>,
}

impl MapperSpillful {
    pub fn new() -> MapperSpillful {
        MapperSpillful { p4: Unique::new(table::P4).unwrap() } // cannot panic, we know P4 is valid.
    }

    pub fn p4(&self) -> &Table<Level4> {
        unsafe { self.p4.as_ref() }
    }

    pub fn p4_mut(&mut self) -> &mut Table<Level4> {
        unsafe { self.p4.as_mut() }
    }

    /// translates a VirtualAddress to a PhysicalAddress
    pub fn translate(&self, virtual_address: VirtualAddress) -> Option<PhysicalAddress> {
        let offset = virtual_address % PAGE_SIZE;
        // get the frame number of the page containing the given virtual address,
        // and then the corresponding physical address is that PFN*sizeof(Page) + offset
        self.translate_page(Page::containing_address(virtual_address)).map(|frame| {
            frame.number * PAGE_SIZE + offset
        })
    }

    pub fn translate_page(&self, page: Page) -> Option<Frame> {
        let p3 = self.p4().next_table(page.p4_index());

        let huge_page = || {
            p3.and_then(|p3| {
                let p3_entry = &p3[page.p3_index()];
                // 1GiB page?
                if let Some(start_frame) = p3_entry.pointed_frame() {
                    if p3_entry.flags().contains(EntryFlags::HUGE_PAGE) {
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
                        if p2_entry.flags().contains(EntryFlags::HUGE_PAGE) {
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


    pub fn map<A>(&mut self, vaddr: VirtualAddress, size: usize, flags: EntryFlags, allocator: &mut A) -> Result<(), &'static str>
        where A: FrameAllocator
    {
        // P4, P3, and P2 entries should never set NO_EXECUTE, only the lowest-level P1 entry should. 
        let mut top_level_flags = flags.clone();
        top_level_flags.set(EntryFlags::NO_EXECUTE, false);
        // top_level_flags.set(EntryFlags::WRITABLE, true); // is the same true for the WRITABLE bit?

        for page in PageRange::from_virt_addr(vaddr, size).clone() {
            let frame = allocator.allocate_frame().ok_or("MapperSpillful::map() -- out of memory trying to alloc frame")?;
            let mut p3 = self.p4_mut().next_table_create(page.p4_index(), top_level_flags, allocator);
            let mut p2 = p3.next_table_create(page.p3_index(), top_level_flags, allocator);
            let mut p1 = p2.next_table_create(page.p2_index(), top_level_flags, allocator);

            if !p1[page.p1_index()].is_unused() {
                error!("MapperSpillful::map() page {:#x} -> frame {:#X}, page was already in use!", page.start_address(), frame.start_address());
                return Err("page was already mapped");
            }
            p1[page.p1_index()].set(frame, flags | EntryFlags::PRESENT);
        }

        VMAS.lock().push(VirtualMemoryArea::new(vaddr, size, flags, ""));
        Ok(())
    }


    pub fn remap(&mut self, vaddr: VirtualAddress, new_flags: EntryFlags) -> Result<(), &'static str> {
        let mut vmas = VMAS.lock();

        let mut vma: Option<&mut VirtualMemoryArea> = None;
        for v in vmas.iter_mut() {
            let start_addr = v.start_address();
            let size = v.size();
            if vaddr >= start_addr && vaddr <= (start_addr + size) {
                vma = Some(v);
            }
        }
        let vma = vma.ok_or("couldn't find corresponding VMA")?;


        if new_flags == vma.flags {
            trace!("remap(): new_flags were the same as existing flags, doing nothing.");
            return Ok(());
        }

        let pages = PageRange::from_virt_addr(vma.start_address(), vma.size());

        let broadcast_tlb_shootdown = BROADCAST_TLB_SHOOTDOWN_FUNC.try();
        let mut vaddrs: Vec<VirtualAddress> = if broadcast_tlb_shootdown.is_some() {
            Vec::with_capacity(pages.size_in_pages())
        } else {
            Vec::new() // avoids allocation if we're not going to use it
        };

        for page in pages {
            let p1 = self.p4_mut()
                .next_table_mut(page.p4_index())
                .and_then(|p3| p3.next_table_mut(page.p3_index()))
                .and_then(|p2| p2.next_table_mut(page.p2_index()))
                .ok_or("mapping code does not support huge pages")?;
            
            let frame = try!(p1[page.p1_index()].pointed_frame().ok_or("remap(): page not mapped"));
            p1[page.p1_index()].set(frame, new_flags | EntryFlags::PRESENT);

            let vaddr = page.start_address();
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            x86_64::instructions::tlb::flush(x86_64::VirtualAddress(vaddr));
            #[cfg(any(target_arch = "aarch64"))]
            aarch64::instructions::tlb::flush(aarch64::VirtualAddress(vaddr));
            if broadcast_tlb_shootdown.is_some() && vaddr != TEMPORARY_PAGE_FRAME {
                vaddrs.push(vaddr);
            }
        }
        
        if let Some(func) = broadcast_tlb_shootdown {
            func(vaddrs);
        }

        vma.flags = new_flags;
        Ok(())
    }


    /// Remove the virtual memory mapping for the given virtual address.
    pub fn unmap<A>(&mut self, vaddr: VirtualAddress, _allocator: &mut A) -> Result<(), &'static str>
        where A: FrameAllocator
    {
        
        let mut vmas = VMAS.lock();
        let (pages, vma_index) = {
            let mut vma_index: Option<usize> = None;
            let mut vma: Option<&VirtualMemoryArea> = None;

            for (i, v) in vmas.iter().enumerate() {
                let start_addr = v.start_address();
                let size = v.size();
                if vaddr >= start_addr && vaddr <= (start_addr + size) {
                    vma = Some(v);
                    vma_index = Some(i);
                }
            }
            let vma = vma.ok_or("couldn't find corresponding VMA")?;
            
            (
                PageRange::from_virt_addr(vma.start_address(), vma.size()),
                vma_index.ok_or("couldn't find corresponding VMA")?
            )
        };


        let broadcast_tlb_shootdown = BROADCAST_TLB_SHOOTDOWN_FUNC.try();
        let mut vaddrs: Vec<VirtualAddress> = if broadcast_tlb_shootdown.is_some() {
            Vec::with_capacity(pages.size_in_pages())
        } else {
            Vec::new() // avoids allocation if we're not going to use it
        };

        for page in pages {
            let p1 = self.p4_mut()
                .next_table_mut(page.p4_index())
                .and_then(|p3| p3.next_table_mut(page.p3_index()))
                .and_then(|p2| p2.next_table_mut(page.p2_index()))
                .ok_or("mapping code does not support huge pages")?;
            
            let _frame = p1[page.p1_index()].pointed_frame().ok_or("unmap(): page not mapped")?;
            p1[page.p1_index()].set_unused();

            let vaddr = page.start_address();
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            x86_64::instructions::tlb::flush(x86_64::VirtualAddress(vaddr));
            #[cfg(any(target_arch = "aarch64"))]
            aarch64::instructions::tlb::flush(aarch64::VirtualAddress(vaddr));
            if broadcast_tlb_shootdown.is_some() && vaddr != TEMPORARY_PAGE_FRAME {
                vaddrs.push(vaddr);
            }
        }
        
        if let Some(func) = broadcast_tlb_shootdown {
            func(vaddrs);
        }

        vmas.remove(vma_index);
        Ok(())
    }
}
