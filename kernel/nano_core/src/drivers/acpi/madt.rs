use core::mem;
use core::intrinsics::{atomic_load, atomic_store};
use core::ops::DerefMut;
use memory::{Stack, FRAME_ALLOCATOR, MappedPages, MemoryManagementInfo, Frame, PageTable, ActivePageTable, Page, PhysicalAddress, VirtualAddress, EntryFlags}; 
use interrupts::ioapic;
use interrupts::apic::{LocalApic, has_x2apic, get_my_apic_id, is_bsp, get_bsp_id};
use kernel_config::memory::PAGE_SHIFT;
use spin::RwLock;
use alloc::boxed::Box;

use super::sdt::Sdt;
use super::{AP_STARTUP, TRAMPOLINE, find_sdt, load_table, get_sdt_signature};

// use core::intrinsics::{atomic_load, atomic_store};
use core::sync::atomic::Ordering;

// use device::local_apic::LOCAL_APIC;
// use interrupt;
use start::{kstart_ap, AP_READY_FLAG};





/// The Multiple APIC Descriptor Table
#[derive(Debug)]
pub struct Madt {
    sdt: &'static Sdt,
    pub local_address: u32,
    pub flags: u32,
}

impl Madt {
    pub fn init(active_table: &mut ActivePageTable) -> Result<MadtIter, &'static str> {
        assert_has_not_been_called!("Madt::init() was called more than once! It should only be called by the bootstrap processor (bsp).");
        
        if !is_bsp() {
            error!("You can only call Madt::init() from the bootstrap processor (bsp), not other cores!");
            return Err("Cannot call Madt::init() from non-bsp cores");
        }
        
        let madt_sdt = find_sdt("APIC");
        if madt_sdt.len() == 1 {
            load_table(get_sdt_signature(madt_sdt[0]));
            let madt = try!(Madt::new(madt_sdt[0]).ok_or("Couldn't parse MADT (APIC) table, it was invalid."));
            let iter = madt.iter();
            try!(handle_ioapic_entry(iter.clone(), active_table));
            try!(handle_bsp_entry(iter.clone()));
            Ok(iter)
        } else {
            error!("Unable to find MADT");
            Err("could not find MADT table (signature 'APIC')")
        }
    }

    fn new(sdt: &'static Sdt) -> Option<Madt> {
        if &sdt.signature == b"APIC" && sdt.data_len() >= 8 { //Not valid if no local address and flags
            let local_address = unsafe { *(sdt.data_address() as *const u32) };
            let flags = unsafe { *(sdt.data_address() as *const u32).offset(1) };

            Some(Madt {
                sdt: sdt,
                local_address: local_address,
                flags: flags
            })
        } else {
            None
        }
    }

    fn iter(&self) -> MadtIter {
        MadtIter {
            sdt: self.sdt,
            i: 8 // Skip local controller address and flags
        }
    }
}


fn handle_ioapic_entry(madt_iter: MadtIter, active_table: &mut ActivePageTable) -> Result<(), &'static str> {
    let mut ioapic_count = 0;
    for madt_entry in madt_iter {
        match madt_entry {
            MadtEntry::IoApic(ioa) => {
                ioapic_count += 1;
                try!(ioapic::init(active_table, ioa.id, ioa.address as usize, ioa.gsi_base));
            }
            // we only handle IoApic entries here
            _ => { }
        }
    }

    if ioapic_count == 1 {
        Ok(())
    }
    else {
        error!("We need exactly 1 IoApic (found {}), cannot support more than 1, or 0 IoApics.", ioapic_count);
        Err("Found more than one IoApic")
    }
}


fn handle_bsp_entry(madt_iter: MadtIter) -> Result<(), &'static str> {
    let all_lapics = ::interrupts::apic::get_lapics();
    let me = try!(get_my_apic_id().ok_or("Couldn't get_my_apic_id"));

    let mut ioapic_locked = ioapic::get_ioapic();
    let ioapic_ref = try!(ioapic_locked.as_mut().ok_or("Couldn't get ioapic_ref!"));


    for madt_entry in madt_iter.clone() {
        match madt_entry {
             MadtEntry::LocalApic(lapic_madt) => { 
                if lapic_madt.apic_id == me {
                    debug!("        This is my (the BSP's) local APIC");
                    // For the BSP's own processor core, no real work is needed. 
                    let mut bsp_lapic = LocalApic::new(lapic_madt.processor, lapic_madt.apic_id, lapic_madt.flags, true, madt_iter.clone());
                    let bsp_id = bsp_lapic.id();

                    use interrupts::PIC_MASTER_OFFSET;
                    // set the BSP to receive regular PIC interrupts routed through the IoApic
                    ioapic_ref.set_irq(0x0, bsp_id, PIC_MASTER_OFFSET + 0x0);
                    ioapic_ref.set_irq(0x1, bsp_id, PIC_MASTER_OFFSET + 0x1); // keyboard interrupt 0x1 -> 0x21 in IDT
                    // skip irq 2, since in the PIC that's the chained one (cascade line from PIC2 to PIC1) that isn't used
                    ioapic_ref.set_irq(0x3, bsp_id, PIC_MASTER_OFFSET + 0x3);
                    ioapic_ref.set_irq(0x4, bsp_id, PIC_MASTER_OFFSET + 0x4);
                    ioapic_ref.set_irq(0x5, bsp_id, PIC_MASTER_OFFSET + 0x5);
                    ioapic_ref.set_irq(0x6, bsp_id, PIC_MASTER_OFFSET + 0x6);
                    ioapic_ref.set_irq(0x7, bsp_id, PIC_MASTER_OFFSET + 0x7);
                    ioapic_ref.set_irq(0x8, bsp_id, PIC_MASTER_OFFSET + 0x8);
                    ioapic_ref.set_irq(0x9, bsp_id, PIC_MASTER_OFFSET + 0x9);
                    ioapic_ref.set_irq(0xa, bsp_id, PIC_MASTER_OFFSET + 0xa);
                    ioapic_ref.set_irq(0xb, bsp_id, PIC_MASTER_OFFSET + 0xb);
                    ioapic_ref.set_irq(0xc, bsp_id, PIC_MASTER_OFFSET + 0xc);
                    ioapic_ref.set_irq(0xd, bsp_id, PIC_MASTER_OFFSET + 0xd);
                    ioapic_ref.set_irq(0xe, bsp_id, PIC_MASTER_OFFSET + 0xe);
                    ioapic_ref.set_irq(0xf, bsp_id, PIC_MASTER_OFFSET + 0xf);
                    // ioapic_ref.set_irq(0x1, 0xFF, PIC_MASTER_OFFSET + 0x1); 
                    // FIXME: the above line does indeed send the interrupt to all cores, but then they all handle it, instead of just one. 
                    
                    // add the BSP lapic to the list (should be empty until here)
                    assert!(all_lapics.iter().next().is_none(), "LocalApics list wasn't empty when adding BSP!! BSP must be the first core added.");
                    all_lapics.insert(lapic_madt.processor, RwLock::new(bsp_lapic));

                    // there's only ever one BSP, so we can exit the loop here
                    break;
                }
            }
            _ => { }
        }
    }

    let bsp_id = try!(get_bsp_id().ok_or("Couldn't find BSP LocalApic in Madt!"));

    // now that we've established the BSP,  go through the interrupt source override entries
    for madt_entry in madt_iter {
        match madt_entry {
            MadtEntry::IntSrcOverride(int_src) => {
                assert!(int_src.gsi <= (u8::max_value() as u32), "Unsupported: gsi value is larger than size of u8: {:?}", int_src);
                // using BSP for now, but later we could redirect the IRQ to more (or all) cores
                use interrupts::PIC_MASTER_OFFSET;
                ioapic_ref.set_irq(int_src.irq_source, bsp_id, int_src.gsi as u8 + PIC_MASTER_OFFSET); 
            } 
            _ => { }
        }
    }

    Ok(())
}



/// Starts up and sets up AP cores based on the given APIIC system table (`madt_iter`)
pub fn handle_ap_cores(madt_iter: MadtIter, kernel_mmi: &mut MemoryManagementInfo) -> Result<usize, &'static str> {
    // SAFE: just getting const values from boot assembly code
    debug!("ap_start_realmode code start: {:#x}, end: {:#x}", ::get_ap_start_realmode(), ::get_ap_start_realmode_end());
    let ap_startup_size_in_bytes = ::get_ap_start_realmode_end() - ::get_ap_start_realmode();

    let active_table_phys_addr: PhysicalAddress;
    let _trampoline_mapped_page: MappedPages; // must be held until APs are booted up
    let ap_startup_mapped_pages: MappedPages; // must be held until APs are booted up

    {
        let &mut MemoryManagementInfo { 
            page_table: ref mut kernel_page_table, 
            ..  // don't need to access the kernel's vmas or stack allocator, we already allocated a kstack above
        } = kernel_mmi;

        match kernel_page_table {
            &mut PageTable::Active(ref mut active_table) => {
                // first, double check that the ap_start_realmode address is mapped and valid
                try!(active_table.translate(::get_ap_start_realmode()).ok_or("handle_ap_cores(): couldn't translate ap_start_realmode address"));

                let mut allocator = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get FRAME ALLOCATOR")).lock();

                // Map trampoline frame and the ap_startup code to the AP_STARTUP frame
                let trampoline_page = Page::containing_address(TRAMPOLINE);
                let trampoline_frame = Frame::containing_address(TRAMPOLINE);
                
                _trampoline_mapped_page = try!( active_table.map_to(
                    trampoline_page, trampoline_frame, EntryFlags::PRESENT | EntryFlags::WRITABLE, allocator.deref_mut())
                );
                ap_startup_mapped_pages = try!( active_table.map_frames(
                    Frame::range_inclusive_addr(AP_STARTUP, ap_startup_size_in_bytes),
                    Page::containing_address(AP_STARTUP),
                    EntryFlags::PRESENT | EntryFlags::WRITABLE, 
                    allocator.deref_mut())
                );

                active_table_phys_addr = active_table.physical_address();
            }
            _ => {
                error!("handle_ap_cores(): couldn't get kernel's active_table!");
                return Err("Couldn't get kernel's active_table");
            }
        }
    }

    let all_lapics = ::interrupts::apic::get_lapics();
    let me = try!(get_my_apic_id().ok_or("Couldn't get_my_apic_id"));

    if ::interrupts::apic::has_x2apic() {
        debug!("Handling APIC (lapic Madt) tables, me: {}, x2apic yes.", me);
    } else {
        debug!("Handling APIC (lapic Madt) tables, me: {}, no x2apic", me);
    }
    
    // we mapped and/or checked the src/dest pointers/addresses earlier
    let src_ptr = ::get_ap_start_realmode() as VirtualAddress as *const u8;
    let dest_ptr = ap_startup_mapped_pages.start_address() as *mut u8; // we mapped this above
    debug!("copying ap_startup code to AP_STARTUP, {} bytes", ap_startup_size_in_bytes);
    use core::ptr::copy_nonoverlapping; // just like memcpy
    // obviously unsafe, but we've mapped everything 
    unsafe {
        copy_nonoverlapping(src_ptr, dest_ptr, ap_startup_size_in_bytes);
    }
    // now, the ap startup code should be at paddr AP_STARTUP


    let mut ap_count = 0;

    // in this function, we only handle LocalApics
    for madt_entry in madt_iter.clone() {
        debug!("      {:?}", madt_entry);
        match madt_entry {
            MadtEntry::LocalApic(lapic_madt) => { 

                if lapic_madt.apic_id == me {
                    debug!("        skipping BSP's local apic");
                }
                else {
                    debug!("        This is a different AP's APIC");
                    // start up this AP, and have it create a new LocalApic for itself. 
                    // This must be done by each core itself, and not called repeatedly by the BSP on behalf of other cores.
                    let bsp_lapic_ref = try!(get_bsp_id().and_then( |bsp_id|  all_lapics.get(&bsp_id)).ok_or("Couldn't get BSP's LocalApic!"));
                    let mut bsp_lapic = bsp_lapic_ref.write();
                    let ap_stack = try!(kernel_mmi.alloc_stack(4).ok_or("could not allocate AP stack!"));
                    bring_up_ap(bsp_lapic.deref_mut(), lapic_madt, active_table_phys_addr, ap_stack, madt_iter.clone());
                    ap_count += 1;
                }
            }
            // only care about new local apics right now
            _ => { }
        }
    }

    Ok(ap_count)  
}



/// Called by the BSP to initialize the given `new_lapic` using IPIs.
fn bring_up_ap(bsp_lapic: &mut LocalApic,
               new_lapic: &MadtLocalApic, 
               active_table_paddr: PhysicalAddress, 
               ap_stack: Stack,
               madt_iter: MadtIter) 
{
    
    let ap_ready = TRAMPOLINE as *mut u64;
    let ap_processor_id = unsafe { ap_ready.offset(1) };
    let ap_apic_id = unsafe { ap_ready.offset(2) };
    let ap_flags = unsafe { ap_ready.offset(3) };
    let ap_page_table = unsafe { ap_ready.offset(4) };
    let ap_stack_start = unsafe { ap_ready.offset(5) };
    let ap_stack_end = unsafe { ap_ready.offset(6) };
    let ap_code = unsafe { ap_ready.offset(7) };
    let ap_madt_table = unsafe { ap_ready.offset(8) };

    // Set the ap_ready to 0, volatile
    unsafe { atomic_store(ap_ready, 0) };
    unsafe { atomic_store(ap_processor_id, new_lapic.processor as u64) };
    unsafe { atomic_store(ap_apic_id, new_lapic.apic_id as u64) };
    unsafe { atomic_store(ap_flags, new_lapic.flags as u64) };
    unsafe { atomic_store(ap_page_table, active_table_paddr as u64) };
    unsafe { atomic_store(ap_stack_start, ap_stack.bottom() as u64) };
    unsafe { atomic_store(ap_stack_end, ap_stack.top_unusable() as u64) };
    unsafe { atomic_store(ap_code, kstart_ap as u64) };
    unsafe { atomic_store(ap_madt_table, &madt_iter as *const _ as u64) };
    AP_READY_FLAG.store(false, Ordering::SeqCst);

    // put the ap_stack on the heap and "leak" it so it's not dropped and auto-unmapped
    Box::into_raw(Box::new(ap_stack)); 

    info!("Bringing up AP, proc: {} apic_id: {}", new_lapic.processor, new_lapic.apic_id);
    let new_apic_id = new_lapic.apic_id; 
    
    // Send INIT IPI
    {
        let mut icr = 0x4500; // 0x500 means INIT Delivery Mode, 0x4000 means Assert Level (not de-assert)
        if has_x2apic() {
            icr |= (new_apic_id as u64) << 32;
        } else {
            icr |= ( new_apic_id as u64) << 56; // destination apic id 
        }
        // icr |= 1 << 11; // (1 << 11) is logical address mode, 0 is physical. Doesn't work with physical addressing mode!
        // debug!(" IPI...");
        bsp_lapic.set_icr(icr);
    }

    // debug!("waiting 10 ms...");
    wait10ms();
    // debug!("done waiting.");

    // Send START IPI
    {
        //Start at 0x0800:0000 => 0x8000. We copied the ap_start_realmode code into AP_STARTUP earlier, in handle_apic_entry()
        let ap_segment = (AP_STARTUP >> PAGE_SHIFT) & 0xFF; // the frame number where we want the AP to start executing from boot
        let mut icr = 0x4600 | ap_segment as u64;

        if has_x2apic() {
            icr |= (new_apic_id as u64) << 32;
        } else {
            icr |= (new_apic_id as u64) << 56;
        }
        // icr |= 1 << 11; // (1 << 11) is logical address mode, 0 is physical. Doesn't work with physical addressing mode!
        // debug!(" SIPI...");
        bsp_lapic.set_icr(icr);
    }


    // TODO: we may need to send a second START IPI on real hardware???

    // Wait for trampoline ready
    // debug!(" Wait...");
    while unsafe { atomic_load(ap_ready) } == 0 {
        ::arch::pause();
    }
    // debug!(" Trampoline...");
    while ! AP_READY_FLAG.load(Ordering::SeqCst) {
        ::arch::pause();
    }
    info!(" AP {} is in Rust code. Ready!", new_apic_id);

}

/// super shitty approximation, busy wait to wait a random bit of time.
/// probably closer to a whole second than just 10ms
fn wait10ms() {
    let mut i = 10000000; 
    while i > 0 {
        i -= 1;
    }
}

/// MADT Local APIC
#[derive(Debug)]
#[repr(packed)]
pub struct MadtLocalApic {
    /// Processor ID
    pub processor: u8,
    /// Local APIC ID
    pub apic_id: u8,
    /// Flags. 1 means that the processor is enabled
    pub flags: u32
}

/// MADT I/O APIC
#[derive(Debug)]
#[repr(packed)]
pub struct MadtIoApic {
    /// I/O APIC ID
    pub id: u8,
    /// reserved
    reserved: u8,
    /// I/O APIC address
    pub address: u32,
    /// Global system interrupt base
    pub gsi_base: u32
}

/// MADT Interrupt Source Override
#[derive(Debug)]
#[repr(packed)]
pub struct MadtIntSrcOverride {
    /// Bus Source
    pub bus_source: u8,
    /// IRQ Source
    pub irq_source: u8,
    /// Global system interrupt
    pub gsi: u32,
    /// Flags
    pub flags: u16
}

/// MADT Non-maskable Interrupt.
/// Configure these with the LINT0 and LINT1 entries in the Local vector table
///  of the relevant processor's (or processors') local APIC.
#[derive(Debug)]
#[repr(packed)]
pub struct MadtNonMaskableInterrupt {
    /// which processor this is for, 0xFF means all processors
    pub processor: u8,
    /// Flags
    pub flags: u16,
    /// LINT (either 0 or 1)
    pub lint: u8,
}

/// MADT Entries
#[derive(Debug)]
pub enum MadtEntry {
    LocalApic(&'static MadtLocalApic),
    InvalidLocalApic(usize),
    IoApic(&'static MadtIoApic),
    InvalidIoApic(usize),
    IntSrcOverride(&'static MadtIntSrcOverride),
    InvalidIntSrcOverride(usize),
    NonMaskableInterrupt(&'static MadtNonMaskableInterrupt),
    InvalidNonMaskableInterrupt(usize),
    Unknown(u8)
}

#[derive(Clone)]
pub struct MadtIter {
    sdt: &'static Sdt,
    i: usize
}

impl Iterator for MadtIter {
    type Item = MadtEntry;
    fn next(&mut self) -> Option<Self::Item> {
        if self.i + 1 < self.sdt.data_len() {
            let entry_type = unsafe { *(self.sdt.data_address() as *const u8).offset(self.i as isize) };
            let entry_len = unsafe { *(self.sdt.data_address() as *const u8).offset(self.i as isize + 1) } as usize;

            if self.i + entry_len <= self.sdt.data_len() {
                let item = match entry_type {
                    0 => if entry_len == mem::size_of::<MadtLocalApic>() + 2 {
                        MadtEntry::LocalApic(unsafe { &*((self.sdt.data_address() + self.i + 2) as *const MadtLocalApic) })
                    } else {
                        MadtEntry::InvalidLocalApic(entry_len)
                    },

                    1 => if entry_len == mem::size_of::<MadtIoApic>() + 2 {
                        MadtEntry::IoApic(unsafe { &*((self.sdt.data_address() + self.i + 2) as *const MadtIoApic) })
                    } else {
                        MadtEntry::InvalidIoApic(entry_len)
                    },

                    2 => if entry_len == mem::size_of::<MadtIntSrcOverride>() + 2 {
                        MadtEntry::IntSrcOverride(unsafe { &*((self.sdt.data_address() + self.i + 2) as *const MadtIntSrcOverride) })
                    } else {
                        MadtEntry::InvalidIntSrcOverride(entry_len)
                    },

                    // Entry Type 3 doesn't exist

                    4 => if entry_len == mem::size_of::<MadtNonMaskableInterrupt>() + 2 {
                        MadtEntry::NonMaskableInterrupt(unsafe { &*((self.sdt.data_address() + self.i + 2) as *const MadtNonMaskableInterrupt) })
                    } else {
                        MadtEntry::InvalidNonMaskableInterrupt(entry_len)
                    },

                    _ => MadtEntry::Unknown(entry_type)
                };

                self.i += entry_len;

                Some(item)
            } else {
                None
            }
        } else {
            None
        }
    }
}
