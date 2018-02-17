// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use x86_64;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::idt::{LockedIdt, ExceptionStackFrame};
use spin::{Mutex, Once};
use port_io::Port;
use drivers::input::keyboard;
use drivers::ata_pio;
use kernel_config::time::{CONFIG_PIT_FREQUENCY_HZ, CONFIG_RTC_FREQUENCY_HZ};
use x86_64::structures::gdt::SegmentSelector;
use rtc;
use core::sync::atomic::{AtomicUsize, Ordering};
use atomic::{Atomic};
use atomic_linked_list::atomic_map::AtomicMap;
use memory::VirtualAddress;

use drivers::e1000;


mod exceptions;
mod gdt;
pub mod pit_clock; // TODO: shouldn't be pub
pub mod apic;
pub mod ioapic;
mod pic;
pub mod tsc;


// re-expose these functions from within this interrupt module
pub use irq_safety::{disable_interrupts, enable_interrupts, interrupts_enabled};
pub use self::exceptions::init_early_exceptions;

/// The index of the double fault stack in a TaskStateSegment (TSS)
const DOUBLE_FAULT_IST_INDEX: usize = 0;


static KERNEL_CODE_SELECTOR:  Once<SegmentSelector> = Once::new();
static KERNEL_DATA_SELECTOR:  Once<SegmentSelector> = Once::new();
static USER_CODE_32_SELECTOR: Once<SegmentSelector> = Once::new();
static USER_DATA_32_SELECTOR: Once<SegmentSelector> = Once::new();
static USER_CODE_64_SELECTOR: Once<SegmentSelector> = Once::new();
static USER_DATA_64_SELECTOR: Once<SegmentSelector> = Once::new();
static TSS_SELECTOR:          Once<SegmentSelector> = Once::new();


/// The single system-wide IDT
/// Note: this could be per-core instead of system-wide, if needed.
pub static IDT: LockedIdt = LockedIdt::new();

/// Interface to our PIC (programmable interrupt controller) chips.
/// We want to map hardware interrupts to 0x20 (for PIC1) or 0x28 (for PIC2).
static PIC: Once<pic::ChainedPics> = Once::new();
static KEYBOARD: Mutex<Port<u8>> = Mutex::new(Port::new(0x60));

/// The TSS list, one per core, indexed by a key of apic_id
lazy_static! {
    static ref TSS: AtomicMap<u8, Mutex<TaskStateSegment>> = AtomicMap::new();
}
/// The GDT list, one per core, indexed by a key of apic_id
lazy_static! {
    static ref GDT: AtomicMap<u8, gdt::Gdt> = AtomicMap::new();
}

pub static INTERRUPT_CHIP: Atomic<InterruptChip> = Atomic::new(InterruptChip::APIC);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum InterruptChip {
    APIC,
    x2apic,
    PIC,
}

pub enum AvailableSegmentSelector {
    KernelCode,
    KernelData,
    UserCode32,
    UserData32,
    UserCode64,
    UserData64,
    Tss,
}


/// Stupid hack because SegmentSelector is not Cloneable/Copyable
pub fn get_segment_selector(selector: AvailableSegmentSelector) -> SegmentSelector {
    let seg: &SegmentSelector = match selector {
        AvailableSegmentSelector::KernelCode => {
            KERNEL_CODE_SELECTOR.try().expect("KERNEL_CODE_SELECTOR wasn't yet inited!")
        }
        AvailableSegmentSelector::KernelData => {
            KERNEL_DATA_SELECTOR.try().expect("KERNEL_DATA_SELECTOR wasn't yet inited!")
        }
        AvailableSegmentSelector::UserCode32 => {
            USER_CODE_32_SELECTOR.try().expect("USER_CODE_32_SELECTOR wasn't yet inited!")
        }
        AvailableSegmentSelector::UserData32 => {
            USER_DATA_32_SELECTOR.try().expect("USER_DATA_32_SELECTOR wasn't yet inited!")
        }
        AvailableSegmentSelector::UserCode64 => {
            USER_CODE_64_SELECTOR.try().expect("USER_CODE_64_SELECTOR wasn't yet inited!")
        }
        AvailableSegmentSelector::UserData64 => {
            USER_DATA_64_SELECTOR.try().expect("USER_DATA_64_SELECTOR wasn't yet inited!")
        }
        AvailableSegmentSelector::Tss => {
            TSS_SELECTOR.try().expect("TSS_SELECTOR wasn't yet inited!")
        }
    };

    SegmentSelector::new(seg.index(), seg.rpl())
}




/// Sets the current core's TSS privilege stack 0 (RSP0) entry, which points to the stack that 
/// the x86_64 hardware automatically switches to when transitioning from Ring 3 -> Ring 0.
/// Should be set to an address within the current userspace task's kernel stack.
/// WARNING: If set incorrectly, the OS will crash upon an interrupt from userspace into kernel space!!
pub fn tss_set_rsp0(new_privilege_stack_top: usize) -> Result<(), &'static str> {
    let my_apic_id = try!(apic::get_my_apic_id().ok_or("couldn't get_my_apic_id"));
    let mut tss_entry = try!(TSS.get(&my_apic_id).ok_or_else(|| {
        error!("tss_set_rsp0(): couldn't find TSS for apic {}", my_apic_id);
        "No TSS for the current core's apid id" 
    })).lock();
    tss_entry.privilege_stack_table[0] = x86_64::VirtualAddress(new_privilege_stack_top);
    // trace!("tss_set_rsp0: new TSS {:?}", tss_entry);
    Ok(())
}



/// initializes the interrupt subsystem and properly sets up safer exception-related IRQs, but no other IRQ handlers.
/// Arguments: the address of the top of a newly allocated stack, to be used as the double fault exception handler stack 
/// Arguments: the address of the top of a newly allocated stack, to be used as the privilege stack (Ring 3 -> Ring 0 stack)
pub fn init(double_fault_stack_top_unusable: VirtualAddress, privilege_stack_top_unusable: VirtualAddress) -> Result<(), &'static str> {

    init_early_exceptions(); // this was probably already done earlier, but it doesn't hurt to make sure

    let bsp_id = try!(apic::get_bsp_id().ok_or("couldn't get BSP's id"));
    info!("Setting up TSS & GDT for BSP (id {})", bsp_id);
    create_tss_gdt(bsp_id, double_fault_stack_top_unusable, privilege_stack_top_unusable);

    // here, we just need to set up special stacks for exceptions, others have already been set up
    {
        let mut idt = IDT.lock(); // withholds interrupts
        unsafe {
            idt.double_fault.set_handler_fn(exceptions::double_fault_handler)
                .set_stack_index(DOUBLE_FAULT_IST_INDEX as u16); // use a special stack for the DF handler
        }
       
        // fill all IDT entries with an unimplemented IRQ handler
        for i in 32..255 {
            idt[i].set_handler_fn(apic_unimplemented_interrupt_handler);
        }
    }

    // try to load our new IDT    
    {
        info!("trying to load IDT...");
        IDT.load();
        info!("loaded interrupt descriptor table.");
    }

    Ok(())

}


pub fn init_ap(apic_id: u8, 
               double_fault_stack_top_unusable: VirtualAddress, 
               privilege_stack_top_unusable: VirtualAddress)
               -> Result<(), &'static str> {
    info!("Setting up TSS & GDT for AP {}", apic_id);
    create_tss_gdt(apic_id, double_fault_stack_top_unusable, privilege_stack_top_unusable);


    info!("trying to load IDT for AP {}...", apic_id);
    IDT.load();
    info!("loaded IDT for AP {}.", apic_id);
    Ok(())
}


fn create_tss_gdt(apic_id: u8, 
                  double_fault_stack_top_unusable: VirtualAddress, 
                  privilege_stack_top_unusable: VirtualAddress) {
    use x86_64::instructions::segmentation::{set_cs, load_ds, load_ss};
    use x86_64::instructions::tables::load_tss;
    use x86_64::PrivilegeLevel;

    // set up TSS and get pointer to it    
    let tss_ref = {
        let mut tss = TaskStateSegment::new();
        // TSS.RSP0 is used in kernel space after a transition from Ring 3 -> Ring 0
        tss.privilege_stack_table[0] = x86_64::VirtualAddress(privilege_stack_top_unusable);
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX] = x86_64::VirtualAddress(double_fault_stack_top_unusable);

        // insert into TSS list
        TSS.insert(apic_id, Mutex::new(tss));
        let tss_ref = TSS.get(&apic_id).unwrap(); // safe to unwrap since we just added it to the list
        // debug!("Created TSS for apic {}, TSS: {:?}", apic_id, tss_ref);
        tss_ref
    };
    

    // set up this AP's GDT
    {
        let mut gdt = gdt::Gdt::new();

        // the following order of segments must be preserved: 
        // 0) null descriptor 
        // 1) kernel cs
        // 2) kernel ds
        // 3) user cs 32
        // 4) user ds 32
        // 5) user cs 64
        // 6) user ds 64
        // 7-8) tss
        // DO NOT rearrange the below calls to gdt.add_entry(), x86_64 has **VERY PARTICULAR** rules about this

        let kernel_cs = gdt.add_entry(gdt::Descriptor::kernel_code_segment(), PrivilegeLevel::Ring0);
        KERNEL_CODE_SELECTOR.call_once(|| kernel_cs);
        let kernel_ds = gdt.add_entry(gdt::Descriptor::kernel_data_segment(), PrivilegeLevel::Ring0);
        KERNEL_DATA_SELECTOR.call_once(|| kernel_ds);
        let user_cs_32 = gdt.add_entry(gdt::Descriptor::user_code_32_segment(), PrivilegeLevel::Ring3);
        USER_CODE_32_SELECTOR.call_once(|| user_cs_32);
        let user_ds_32 = gdt.add_entry(gdt::Descriptor::user_data_32_segment(), PrivilegeLevel::Ring3);
        USER_DATA_32_SELECTOR.call_once(|| user_ds_32);
        let user_cs_64 = gdt.add_entry(gdt::Descriptor::user_code_64_segment(), PrivilegeLevel::Ring3);
        USER_CODE_64_SELECTOR.call_once(|| user_cs_64);
        let user_ds_64 = gdt.add_entry(gdt::Descriptor::user_data_64_segment(), PrivilegeLevel::Ring3);
        USER_DATA_64_SELECTOR.call_once(|| user_ds_64);
        use core::ops::Deref;
        let tss = gdt.add_entry(gdt::Descriptor::tss_segment(tss_ref.lock().deref()), PrivilegeLevel::Ring0);
        TSS_SELECTOR.call_once(|| tss);
        
        GDT.insert(apic_id, gdt);
        let gdt_ref = GDT.get(&apic_id).unwrap(); // safe to unwrap since we just added it to the list
        gdt_ref.load();
        // debug!("Loaded GDT for apic {}: {}", apic_id, gdt_ref);
    }

    unsafe {
        set_cs(get_segment_selector(AvailableSegmentSelector::KernelCode)); // reload code segment register
        load_tss(get_segment_selector(AvailableSegmentSelector::Tss)); // load TSS
        
        load_ss(get_segment_selector(AvailableSegmentSelector::KernelData)); // unsure if necessary
        load_ds(get_segment_selector(AvailableSegmentSelector::KernelData)); // unsure if necessary
    }
}

pub fn init_handlers_apic() {
    // first, do the standard interrupt remapping, but mask all PIC interrupts / disable the PIC
    PIC.call_once( || {
        pic::ChainedPics::init(None, None, 0xFF, 0xFF) // disable all PIC IRQs
    });

    {
        let mut idt = IDT.lock(); // withholds interrupts
        
        // exceptions (IRQS from 0 -31) have already been inited before

        // fill all IDT entries with an unimplemented IRQ handler
        for i in 32..255 {
            idt[i].set_handler_fn(apic_unimplemented_interrupt_handler);
        }
        
        idt[0x20].set_handler_fn(pit_timer_handler);
        idt[0x21].set_handler_fn(keyboard_handler);
        idt[0x22].set_handler_fn(lapic_timer_handler);
        idt[0x2B].set_handler_fn(nic_handler);
        idt[apic::APIC_SPURIOUS_INTERRUPT_VECTOR as usize].set_handler_fn(apic_spurious_interrupt_handler); 


        idt[apic::TLB_SHOOTDOWN_IPI_IRQ as usize].set_handler_fn(ipi_handler);
    }


    // now it's safe to enable every LocalApic's LVT_TIMER interrupt (for scheduling)
    
}


pub fn init_handlers_pic() {
    {
        let mut idt = IDT.lock(); // withholds interrupts
		// SET UP CUSTOM INTERRUPT HANDLERS
		// we can directly index the "idt" object because it implements the Index/IndexMut traits

       
        // MASTER PIC starts here (0x20 - 0x27)
        idt[0x20].set_handler_fn(pit_timer_handler);
        idt[0x21].set_handler_fn(keyboard_handler);
        // there is no IRQ 0x22        
        idt[0x23].set_handler_fn(irq_0x23_handler); 
        idt[0x24].set_handler_fn(irq_0x24_handler); 
        idt[0x25].set_handler_fn(irq_0x25_handler); 
        idt[0x26].set_handler_fn(irq_0x26_handler); 

        idt[0x27].set_handler_fn(spurious_interrupt_handler); 


        // SLAVE PIC starts here (0x28 - 0x2E)        
        // idt[0x28].set_handler_fn(rtc_handler); // using the weird way temporarily

        idt[0x29].set_handler_fn(irq_0x29_handler); 
        idt[0x2A].set_handler_fn(irq_0x2A_handler); 
        idt[0x2B].set_handler_fn(irq_0x2B_handler); 
        idt[0x2C].set_handler_fn(irq_0x2C_handler); 
        idt[0x2D].set_handler_fn(irq_0x2D_handler); 

        idt[0x2E].set_handler_fn(primary_ata);
        // 0x2F missing right now

    }

    // init PIC, PIT and RTC interrupts
    let master_pic_mask: u8 = 0x0; // allow every interrupt
    let slave_pic_mask: u8 = 0b0000_1000; // everything is allowed except 0x2B 
    PIC.call_once( || {
        pic::ChainedPics::init(None, None, master_pic_mask, slave_pic_mask) // disable all PIC IRQs
    });

    pit_clock::init(CONFIG_PIT_FREQUENCY_HZ);
    let rtc_handler = rtc::init(CONFIG_RTC_FREQUENCY_HZ, rtc_interrupt_func);
    IDT.lock()[0x28].set_handler_fn(rtc_handler.unwrap());
}






/// Send an end of interrupt signal, which works for all types of interrupt chips (APIC, x2apic, PIC)
/// irq arg is only used for PIC
fn eoi(irq: Option<u8>) {
    match INTERRUPT_CHIP.load(Ordering::Acquire) {
        InterruptChip::APIC |
        InterruptChip::x2apic => {
            apic::get_my_apic().expect("eoi(): couldn't get my apic to send EOI!").read().eoi();
        }
        InterruptChip::PIC => {
            PIC.try().expect("eoi(): PIC not initialized").notify_end_of_interrupt(irq.expect("PIC eoi, but no arg provided"));
        }
    }
}


/// 0x20
extern "x86-interrupt" fn pit_timer_handler(stack_frame: &mut ExceptionStackFrame) {
    pit_clock::handle_timer_interrupt();

	eoi(Some(0x20));
}


/// 0x21
extern "x86-interrupt" fn keyboard_handler(stack_frame: &mut ExceptionStackFrame) {
    // in this interrupt, we must read the keyboard scancode register before acknowledging the interrupt.
    let scan_code: u8 = { 
        KEYBOARD.lock().read() 
    };
	// trace!("APIC KBD (AP {:?}): scan_code {:?}", apic::get_my_apic_id(), scan_code);

    keyboard::handle_keyboard_input(scan_code);	

    eoi(Some(0x21));
}


pub static APIC_TIMER_TICKS: AtomicUsize = AtomicUsize::new(0);
/// 0x22
extern "x86-interrupt" fn lapic_timer_handler(stack_frame: &mut ExceptionStackFrame) {
    let ticks = APIC_TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    // info!(" ({}) APIC TIMER HANDLER! TICKS = {}", apic::get_my_apic_id().unwrap_or(0xFF), ticks);
    
    eoi(None);
    // we must acknowledge the interrupt first before handling it because we context switch here, which doesn't return
    
    schedule!();
}

/// 0x2B
extern "x86-interrupt" fn nic_handler(stack_frame: &mut ExceptionStackFrame) {
    e1000::e1000_handler();
	eoi(Some(0x2B));
}


extern "x86-interrupt" fn apic_spurious_interrupt_handler(stack_frame: &mut ExceptionStackFrame) {
    info!("APIC SPURIOUS INTERRUPT HANDLER!");

    eoi(None);
}

extern "x86-interrupt" fn apic_unimplemented_interrupt_handler(stack_frame: &mut ExceptionStackFrame) {
    println_unsafe!("APIC UNIMPLEMENTED IRQ!!!");

    if let Some(lapic_ref) = apic::get_my_apic() {
        let lapic = lapic_ref.read();
        let isr = lapic.get_isr(); 
        let irr = lapic.get_irr();
        println_unsafe!("APIC ISR: {:#x} {:#x} {:#x} {:#x}, {:#x} {:#x} {:#x} {:#x} \nIRR: {:#x} {:#x} {:#x} {:#x},{:#x} {:#x} {:#x} {:#x}", 
                         isr.0, isr.1, isr.2, isr.3, isr.4, isr.5, isr.6, isr.7, irr.0, irr.1, irr.2, irr.3, irr.4, irr.5, irr.6, irr.7);
    }
    else {
        println_unsafe!("apic_unimplemented_interrupt_handler: couldn't get my apic.");
    }

    loop { }

    eoi(None);
}



pub static mut SPURIOUS_COUNT: u64 = 0;

/// The Spurious interrupt handler. 
/// This has given us a lot of problems on bochs emulator and on some real hardware, but not on QEMU.
/// I believe the problem is something to do with still using the antiquated PIC (instead of APIC)
/// on an SMP system with only one CPU core.
/// See here for more: https://mailman.linuxchix.org/pipermail/techtalk/2002-August/012697.html
/// Thus, for now, we will basically just ignore/ack it, but ideally this will no longer happen
/// when we transition from PIC to APIC, and disable the PIC altogether. 
extern "x86-interrupt" fn spurious_interrupt_handler(stack_frame: &mut ExceptionStackFrame ) {
    unsafe { SPURIOUS_COUNT += 1; } // cheap counter just for debug info

    if let Some(pic) = PIC.try() {
        let irq_regs = pic.read_isr_irr();
        // check if this was a real IRQ7 (parallel port) (bit 7 will be set)
        // (pretty sure this will never happen)
        // if it was a real IRQ7, we do need to ack it by sending an EOI
        if irq_regs.master_isr & 0x80 == 0x80 {
            println_unsafe!("\nGot real IRQ7, not spurious! (Unexpected behavior)");
            warn!("Got real IRQ7, not spurious! (Unexpected behavior)");
            eoi(Some(0x27));
        }
        else {
            // do nothing. Do not send an EOI.
        }
    }
    else {
        error!("spurious_interrupt_handler(): PIC wasn't initialized!");
    }

}



fn rtc_interrupt_func(rtc_ticks: Option<usize>) {
    trace!("rtc_interrupt_func: rtc_ticks = {:?}", rtc_ticks);
}

// //0x28
// extern "x86-interrupt" fn rtc_handler(stack_frame: &mut ExceptionStackFrame ) {
//     // because we use the RTC interrupt handler for context switching,
//     // we must ack the interrupt and send EOI before calling the handler, 
//     // because the handler will not return.
//     rtc::rtc_ack_irq();
//     eoi(Some(0x28));
    
//     rtc::handle_rtc_interrupt();
// }


//0x2e
extern "x86-interrupt" fn primary_ata(stack_frame:&mut ExceptionStackFrame ) {

    ata_pio::handle_primary_interrupt();

    eoi(Some(0x2e));
}


extern "x86-interrupt" fn unimplemented_interrupt_handler(stack_frame: &mut ExceptionStackFrame) {
    let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());    
    println_unsafe!("UNIMPLEMENTED IRQ!!! {:?}", irq_regs);

    loop { }
}


extern "x86-interrupt" fn irq_0x22_handler(stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());    
    println_unsafe!("\nCaught 0x22 interrupt: {:#?}", stack_frame);
    println_unsafe!("IrqRegs: {:?}", irq_regs);

    loop { }
}

extern "x86-interrupt" fn irq_0x23_handler(stack_frame: &mut ExceptionStackFrame) {
    let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
	println_unsafe!("\nCaught 0x23 interrupt: {:#?}", stack_frame);
    println_unsafe!("IrqRegs: {:?}", irq_regs);

    loop { }
}

extern "x86-interrupt" fn irq_0x24_handler(stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());
    println_unsafe!("\nCaught 0x24 interrupt: {:#?}", stack_frame);
    println_unsafe!("IrqRegs: {:?}", irq_regs);

    loop { }
}

extern "x86-interrupt" fn irq_0x25_handler(stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_unsafe!("\nCaught 0x25 interrupt: {:#?}", stack_frame);
    println_unsafe!("IrqRegs: {:?}", irq_regs);

    loop { }
}


extern "x86-interrupt" fn irq_0x26_handler(stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_unsafe!("\nCaught 0x26 interrupt: {:#?}", stack_frame);
    println_unsafe!("IrqRegs: {:?}", irq_regs);

    loop { }
}

extern "x86-interrupt" fn irq_0x29_handler(stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_unsafe!("\nCaught 0x29 interrupt: {:#?}", stack_frame);
    println_unsafe!("IrqRegs: {:?}", irq_regs);

    loop { }
}



extern "x86-interrupt" fn irq_0x2A_handler(stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_unsafe!("\nCaught 0x2A interrupt: {:#?}", stack_frame);
    println_unsafe!("IrqRegs: {:?}", irq_regs);

    loop { }
}


extern "x86-interrupt" fn irq_0x2B_handler(stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_unsafe!("\nCaught 0x2B interrupt: {:#?}", stack_frame);
    println_unsafe!("IrqRegs: {:?}", irq_regs);

    loop { }
}


extern "x86-interrupt" fn irq_0x2C_handler(stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_unsafe!("\nCaught 0x2C interrupt: {:#?}", stack_frame);
    println_unsafe!("IrqRegs: {:?}", irq_regs);

    loop { }
}


extern "x86-interrupt" fn irq_0x2D_handler(stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_unsafe!("\nCaught 0x2D interrupt: {:#?}", stack_frame);
    println_unsafe!("IrqRegs: {:?}", irq_regs);

    loop { }
}



extern "x86-interrupt" fn ipi_handler(stack_frame: &mut ExceptionStackFrame) {
    // Currently, IPIs are only used for TLB shootdowns.
    
    // trace!("ipi_handler (AP {})", apic::get_my_apic_id().unwrap_or(0xFF));
    apic::handle_tlb_shootdown_ipi();

    eoi(None);
}

