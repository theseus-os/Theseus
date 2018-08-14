use core::mem;
use core::ops::DerefMut;
use core::sync::atomic::Ordering;
use core::ptr::{read_volatile, write_volatile};
use alloc::boxed::Box;
use alloc::arc::Arc;
use spin::{Mutex, RwLock};
use kernel_config::memory::{KERNEL_OFFSET, PAGE_SHIFT};
use memory::{Stack, FRAME_ALLOCATOR, Page, MappedPages, MemoryManagementInfo, Frame, PageTable, ActivePageTable, PhysicalAddress, VirtualAddress, EntryFlags}; 
use ioapic;
use apic::{LocalApic, has_x2apic, get_my_apic_id, get_lapics, is_bsp, get_bsp_id};
use irq_safety::MutexIrqSafe;
use pit_clock;

use super::sdt::Sdt;
use super::{AP_STARTUP, TRAMPOLINE, find_sdt, load_table, get_sdt_signature};


use core::sync::atomic::spin_loop_hint;
use ap_start::{kstart_ap, AP_READY_FLAG};

pub static GRAPHIC_INFO:Mutex<GraphicInfo> = Mutex::new(GraphicInfo{
    x:0,
    y:0,
    physical_address:0,
});


/// The Multiple APIC Descriptor Table
#[derive(Debug)]
pub struct Madt {
    sdt: &'static Sdt,
    pub local_address: u32,
    pub flags: u32,
}

impl Madt {
    pub fn init(active_table: &mut ActivePageTable) -> Result<MadtIter, &'static str> {
        
        if !is_bsp() {
            error!("You can only call Madt::init() from the bootstrap processor (bsp), not other cores!");
            return Err("Cannot call Madt::init() from non-bsp cores");
        }
        
        let madt_sdt = find_sdt("APIC");
        if madt_sdt.len() == 1 {
            load_table(get_sdt_signature(madt_sdt[0]));
            let madt = try!(Madt::new(madt_sdt[0]).ok_or("Couldn't parse MADT (APIC) table, it was invalid."));
            let iter = madt.iter();

            // debug!("================ MADT Table: ===============");
            // for e in iter.clone() {
            //     debug!("  {:?}", e);
            // }

            try!(handle_ioapic_entries(iter.clone(), active_table));
            try!(handle_bsp_entry(iter.clone(), active_table));
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


fn handle_ioapic_entries(madt_iter: MadtIter, active_table: &mut ActivePageTable) -> Result<(), &'static str> {
    for madt_entry in madt_iter {
        match madt_entry {
            MadtEntry::IoApic(ioa) => {
                let ioapic = ioapic::IoApic::new(active_table, ioa.id, ioa.address as PhysicalAddress, ioa.gsi_base)?;
                ioapic::get_ioapics().insert(ioa.id, Mutex::new(ioapic));
            }
            // we only handle IoApic entries here
            _ => { }
        }
    }

    Ok(())
}


fn handle_bsp_entry(madt_iter: MadtIter, active_table: &mut ActivePageTable) -> Result<(), &'static str> {
    use pic::PIC_MASTER_OFFSET;

    let all_lapics = get_lapics();
    let me = try!(get_my_apic_id().ok_or("handle_bsp_entry(): Couldn't get_my_apic_id"));


    for madt_entry in madt_iter.clone() {
        if let MadtEntry::LocalApic(lapic_entry) = madt_entry { 
            if lapic_entry.apic_id == me {
                debug!("        This is my (the BSP's) local APIC");
                let (nmi_lint, nmi_flags) = find_nmi_entry_for_processor(lapic_entry.processor, madt_iter.clone());

                let mut bsp_lapic = try!(LocalApic::new(active_table, lapic_entry.processor, lapic_entry.apic_id, true, nmi_lint, nmi_flags));
                let bsp_id = bsp_lapic.id();

                // redirect every IoApic's interrupts to the one BSP
                // TODO FIXME: I'm unsure if this is actually correct!
                for ioapic in ioapic::get_ioapics().iter() {
                    let mut ioapic_ref = ioapic.1.lock();

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
                }
                
                // add the BSP lapic to the list (should be empty until here)
                assert!(all_lapics.iter().next().is_none(), "LocalApics list wasn't empty when adding BSP!! BSP must be the first core added.");
                all_lapics.insert(lapic_entry.apic_id, RwLock::new(bsp_lapic));

                // there's only ever one BSP, so we can exit the loop here
                break;
            }
        }
    }

    let bsp_id = try!(get_bsp_id().ok_or("handle_bsp_entry(): Couldn't find BSP LocalApic in Madt!"));

    // now that we've established the BSP,  go through the interrupt source override entries
    for madt_entry in madt_iter {
        if let MadtEntry::IntSrcOverride(int_src) = madt_entry {
            let mut handled = false;

            // find the IoApic that should handle this interrupt source override entry
            for (_id, ioapic) in ioapic::get_ioapics().iter() {
                let mut ioapic_ref = ioapic.lock();
                if ioapic_ref.handles_irq(int_src.gsi) {
                    // using BSP for now, but later we could redirect the IRQ to more (or all) cores
                    ioapic_ref.set_irq(int_src.irq_source, bsp_id, int_src.gsi as u8 + PIC_MASTER_OFFSET); 
                    trace!("MadtIntSrcOverride (bus: {}, irq: {}, gsi: {}, flags {:#X}) handled by IoApic {}.",
                    int_src.bus_source, int_src.irq_source, int_src.gsi, int_src.flags, ioapic_ref.id);
                    handled = true;
                }
            }

            if !handled {
                error!("MadtIntSrcOverride (bus: {}, irq: {}, gsi: {}, flags {:#X}) not handled by any IoApic!",
                    int_src.bus_source, int_src.irq_source, int_src.gsi, int_src.flags);
            }
        }
    }

    Ok(())
}



/// Starts up and sets up AP cores based on the given APIC system table (`madt_iter`).
/// 
/// Arguments: 
/// 
/// * madt_iter: An iterator over the entries in the MADT APIC table
/// * kernel_mmi_ref: A reference to the locked MMI structure for the kernel.
/// * ap_start_realmode_begin: the starting virtual address of where the ap_start realmode code is.
/// * ap_start_realmode_end: the ending virtual address of where the ap_start realmode code is.
/// 
pub fn handle_ap_cores(madt_iter: MadtIter, kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>,
                       ap_start_realmode_begin: usize, ap_start_realmode_end: usize) -> Result<usize, &'static str> {
    let ap_startup_size_in_bytes = ap_start_realmode_end - ap_start_realmode_begin;

    let active_table_phys_addr: PhysicalAddress;
    let trampoline_mapped_pages: MappedPages; // must be held throughout APs being booted up
    let _trampoline_mapped_pages_higher: MappedPages; // must be held throughout APs being booted up
    let ap_startup_mapped_pages: MappedPages; // must be held throughout APs being booted up
    let _ap_startup_mapped_pages_higher: MappedPages; // must be held throughout APs being booted up

    {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let &mut MemoryManagementInfo { 
            page_table: ref mut kernel_page_table, 
            ..  // don't need to access the kernel's vmas or stack allocator, we already allocated a kstack above
        } = &mut *kernel_mmi;

        match kernel_page_table {
            &mut PageTable::Active(ref mut active_table) => {
                // first, double check that the ap_start_realmode address is mapped and valid
                try!(active_table.translate(ap_start_realmode_begin).ok_or("handle_ap_cores(): couldn't translate ap_start_realmode address"));

                // Map trampoline frame and the ap_startup code to the AP_STARTUP frame.
                // These frames MUST be identity mapped because they're accessed in AP boot up code, which has no page tables.
                let trampoline_page = Page::containing_address(TRAMPOLINE);
                let trampoline_page_higher = Page::containing_address(TRAMPOLINE + KERNEL_OFFSET);
                let trampoline_frame = Frame::containing_address(TRAMPOLINE);
                let ap_startup_page = Page::containing_address(AP_STARTUP);
                let ap_startup_page_higher = Page::containing_address(AP_STARTUP + KERNEL_OFFSET);
                let ap_startup_frames = Frame::range_inclusive_addr(AP_STARTUP, ap_startup_size_in_bytes);

                let mut allocator = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get FRAME ALLOCATOR")).lock();
                
                trampoline_mapped_pages = try!( active_table.map_to(
                    trampoline_page, trampoline_frame.clone(), EntryFlags::PRESENT | EntryFlags::WRITABLE, allocator.deref_mut())
                );
                ap_startup_mapped_pages = try!( active_table.map_frames(
                    ap_startup_frames.clone(), ap_startup_page, EntryFlags::PRESENT | EntryFlags::WRITABLE, allocator.deref_mut())
                );

                // do same mappings for higher half (not sure if needed)
                _trampoline_mapped_pages_higher = try!( active_table.map_to(
                    trampoline_page_higher, trampoline_frame, EntryFlags::PRESENT | EntryFlags::WRITABLE, allocator.deref_mut())
                );
                _ap_startup_mapped_pages_higher = try!( active_table.map_frames(
                    ap_startup_frames, ap_startup_page_higher, EntryFlags::PRESENT | EntryFlags::WRITABLE, allocator.deref_mut())
                );

                active_table_phys_addr = active_table.physical_address();
            }
            _ => {
                error!("handle_ap_cores(): couldn't get kernel's active_table!");
                return Err("Couldn't get kernel's active_table");
            }
        }
    }

    let all_lapics = get_lapics();
    let me = try!(get_my_apic_id().ok_or("Couldn't get_my_apic_id"));

    debug!("Handling APIC (lapic Madt) tables, me: {}, x2apic {}.", me, has_x2apic());
    
    // we checked the src_ptr and mapped the dest_ptr earlier
    let src_ptr = ap_start_realmode_begin as VirtualAddress as *const u8;
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
            MadtEntry::LocalApic(lapic_entry) => { 

                if lapic_entry.apic_id == me {
                    debug!("        skipping BSP's local apic");
                }
                else {
                    debug!("        This is a different AP's APIC");
                    // debug!("        BSP's id: {:?} .....   ALL_LAPICS:", get_bsp_id());
                    // for l in all_lapics.iter() {
                    //     debug!("{:?}", l);
                    // }

                    if lapic_entry.flags & 0x1 != 0x1 {
                        warn!("Processor {} apic_id {} is disabled by the hardware, cannot initialize or be used.", 
                               lapic_entry.processor, lapic_entry.apic_id);
                        continue;
                    }


                    // start up this AP, and have it create a new LocalApic for itself. 
                    // This must be done by each core itself, and not called repeatedly by the BSP on behalf of other cores.
                    let bsp_lapic_ref = try!(get_bsp_id().and_then( |bsp_id|  all_lapics.get(&bsp_id))
                                                         .ok_or("Couldn't get BSP's LocalApic!")
                    );
                    let mut bsp_lapic = bsp_lapic_ref.write();
                    let ap_stack = {
                        let mut kernel_mmi = kernel_mmi_ref.lock();
                        try!(kernel_mmi.alloc_stack(4).ok_or("could not allocate AP stack!"))
                    };

                    let (nmi_lint, nmi_flags) = find_nmi_entry_for_processor(lapic_entry.processor, madt_iter.clone());

                    bring_up_ap(bsp_lapic.deref_mut(), 
                                lapic_entry,
                                trampoline_mapped_pages.start_address(), 
                                active_table_phys_addr, 
                                ap_stack, 
                                nmi_lint,
                                nmi_flags 
                    );
                    ap_count += 1;
                }
            }
            // only care about new local apics right now
            _ => { }
        }
    }

    {    
        let graphic_info = trampoline_mapped_pages.as_type::<GraphicInfo>(0x100).unwrap();
        let mut info = GRAPHIC_INFO.lock();
        *info = GraphicInfo {
            x:graphic_info.x,
            y:graphic_info.y,
            physical_address:graphic_info.physical_address,
        };
    }
    
    // wait for all cores to finish booting and init
    info!("handle_ap_cores(): BSP is waiting for APs to boot...");
    let mut count = get_lapics().iter().count();
    while count < ap_count + 1 {
        trace!("BSP-known count: {}", count);
        spin_loop_hint();
        count = get_lapics().iter().count();
    }
    
    Ok(ap_count)  
}

fn find_nmi_entry_for_processor(processor: u8, madt_iter: MadtIter) -> (u8, u16) {
    for madt_entry in madt_iter {
        match madt_entry {
            MadtEntry::NonMaskableInterrupt(nmi) => {
                // NMI entries are based on the "processor" id, not the "apic_id"
                // Return this Nmi entry if it's for the given lapic, or if it's for all lapics
                if nmi.processor == processor || nmi.processor == 0xFF  {
                    return (nmi.lint, nmi.flags);
                }
            }
            _ => {  }
        }
    }

    let (lint, flags) = (1, 0);
    warn!("Couldn't find NMI entry for processor {} (<-- not apic_id). Using default lint {}, flags {}", processor, lint, flags);
    (lint, flags)
}



/// Called by the BSP to initialize the given `new_lapic` using IPIs.
fn bring_up_ap(bsp_lapic: &mut LocalApic,
               new_lapic: &MadtLocalApic, 
               trampoline_vaddr: VirtualAddress,
               active_table_paddr: PhysicalAddress, 
               ap_stack: Stack,
               nmi_lint: u8, 
               nmi_flags: u16) 
{
    
    // NOTE: These definitions MUST match those in ap_boot.asm
    let ap_ready         = trampoline_vaddr as *mut u64;
    let ap_processor_id  = unsafe { ap_ready.offset(1) };
    let ap_apic_id       = unsafe { ap_ready.offset(2) };
    let ap_page_table    = unsafe { ap_ready.offset(3) };
    let ap_stack_start   = unsafe { ap_ready.offset(4) };
    let ap_stack_end     = unsafe { ap_ready.offset(5) };
    let ap_code          = unsafe { ap_ready.offset(6) };
    let ap_nmi_lint      = unsafe { ap_ready.offset(7) };
    let ap_nmi_flags     = unsafe { ap_ready.offset(8) };

    // Set the ap_ready to 0, volatile
    unsafe { write_volatile(ap_ready,         0) };
    unsafe { write_volatile(ap_processor_id,  new_lapic.processor as u64) };
    unsafe { write_volatile(ap_apic_id,       new_lapic.apic_id as u64) };
    unsafe { write_volatile(ap_page_table,    active_table_paddr as u64) };
    unsafe { write_volatile(ap_stack_start,   ap_stack.bottom() as u64) };
    unsafe { write_volatile(ap_stack_end,     ap_stack.top_unusable() as u64) };
    unsafe { write_volatile(ap_code,          kstart_ap as u64) };
    unsafe { write_volatile(ap_nmi_lint,      nmi_lint as u64) };
    unsafe { write_volatile(ap_nmi_flags,     nmi_flags as u64) };
    AP_READY_FLAG.store(false, Ordering::SeqCst);

    // put the ap_stack on the heap and "leak" it so it's not dropped and auto-unmapped
    Box::into_raw(Box::new(ap_stack)); 

    info!("Bringing up AP, proc: {} apic_id: {}", new_lapic.processor, new_lapic.apic_id);
    let new_apic_id = new_lapic.apic_id; 
    
    bsp_lapic.clear_error();
    let esr = bsp_lapic.error();
    debug!(" initial esr = {:#X}", esr);

    // Send INIT IPI
    {
        // 0x500 means INIT Delivery Mode, 0x4000 means Assert (not de-assert), 0x8000 means level triggers
        let mut icr = /*0x8000 |*/ 0x4000 | 0x500; 
        if has_x2apic() {
            icr |= (new_apic_id as u64) << 32;
        } else {
            icr |= ( new_apic_id as u64) << 56; // destination apic id 
        }
        // icr |= 1 << 11; // (1 << 11) is logical address mode, 0 is physical. Doesn't work with physical addressing mode!
        debug!(" INIT IPI... icr: {:#X}", icr);
        bsp_lapic.set_icr(icr);
    }

    debug!("waiting 10 ms...");
    pit_clock::pit_wait(10000).expect("bring_up_ap(): failed to pit_wait 10 ms");
    debug!("done waiting.");

    // // Send DEASSERT INIT IPI
    // {
    //     // 0x500 means INIT Delivery Mode, 0x8000 means level triggers
    //     let mut icr = 0x8000 | 0x500; 
    //     if has_x2apic() {
    //         icr |= (new_apic_id as u64) << 32;
    //     } else {
    //         icr |= ( new_apic_id as u64) << 56; // destination apic id 
    //     }
    //     // icr |= 1 << 11; // (1 << 11) is logical address mode, 0 is physical. Doesn't work with physical addressing mode!
    //     debug!(" DEASSERT IPI... icr: {:#X}", icr);
    //     bsp_lapic.set_icr(icr);
    // }

    bsp_lapic.clear_error();
    let esr = bsp_lapic.error();
    debug!(" pre-SIPI esr = {:#X}", esr);

    // Send START IPI
    {
        //Start at 0x1000:0000 => 0x10000. We copied the ap_start_realmode code into AP_STARTUP earlier, in handle_apic_entry()
        let ap_segment = (AP_STARTUP >> PAGE_SHIFT) & 0xFF; // the frame number where we want the AP to start executing from boot
        let mut icr = /*0x8000 |*/ 0x4000 | 0x600 | ap_segment as u64; //0x600 means Startup IPI

        if has_x2apic() {
            icr |= (new_apic_id as u64) << 32;
        } else {
            icr |= (new_apic_id as u64) << 56;
        }
        // icr |= 1 << 11; // (1 << 11) is logical address mode, 0 is physical. Doesn't work with physical addressing mode!
        debug!(" SIPI... icr: {:#X}", icr);
        bsp_lapic.set_icr(icr);
    }

    pit_clock::pit_wait(300).expect("bring_up_ap(): failed to pit_wait 300 us");
    pit_clock::pit_wait(200).expect("bring_up_ap(): failed to pit_wait 200 us");

    bsp_lapic.clear_error();
    let esr = bsp_lapic.error();
    debug!(" post-SIPI esr = {:#X}", esr);
    // TODO: we may need to send a second START IPI on real hardware???

    // Wait for trampoline ready
    debug!(" Wait...");
    while unsafe { read_volatile(ap_ready) } == 0 {
        spin_loop_hint();
    }
    debug!(" Trampoline...");
    while ! AP_READY_FLAG.load(Ordering::SeqCst) {
        spin_loop_hint();
    }
    info!(" AP {} is in Rust code. Ready!", new_apic_id);

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

#[derive(Clone, Copy)]
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

pub struct GraphicInfo{
    pub x:u64,
    pub y:u64,
    pub physical_address:u64,
}
