// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::idt::{LockedIdt, ExceptionStackFrame, PageFaultErrorCode};
use spin::{Mutex, Once};
use irq_safety::MutexIrqSafe;
use port_io::Port;
use drivers::input::keyboard;
use drivers::ata_pio;
use kernel_config::time::{CONFIG_PIT_FREQUENCY_HZ, CONFIG_TIMESLICE_PERIOD_MS, CONFIG_RTC_FREQUENCY_HZ};
use x86_64::structures::gdt::SegmentSelector;
use rtc;

// re-expose these functions from within this interrupt module
pub use irq_safety::{disable_interrupts, enable_interrupts, interrupts_enabled};


mod gdt;
pub mod pit_clock; // TODO: shouldn't be pub
pub mod apic;
mod pic;
pub mod tsc;



const DOUBLE_FAULT_IST_INDEX: usize = 0;


static KERNEL_CODE_SELECTOR:  Once<SegmentSelector> = Once::new();
static KERNEL_DATA_SELECTOR:  Once<SegmentSelector> = Once::new();
static USER_CODE_32_SELECTOR: Once<SegmentSelector> = Once::new();
static USER_DATA_32_SELECTOR: Once<SegmentSelector> = Once::new();
static USER_CODE_64_SELECTOR: Once<SegmentSelector> = Once::new();
static USER_DATA_64_SELECTOR: Once<SegmentSelector> = Once::new();
static TSS_SELECTOR:          Once<SegmentSelector> = Once::new();



pub static IDT: LockedIdt = LockedIdt::new();

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
            KERNEL_CODE_SELECTOR.try().expect("KERNEL_CODE_SELECTOR failed to init!")
        }
        AvailableSegmentSelector::KernelData => {
            KERNEL_DATA_SELECTOR.try().expect("KERNEL_DATA_SELECTOR failed to init!")
        }
        AvailableSegmentSelector::UserCode32 => {
            USER_CODE_32_SELECTOR.try().expect("USER_CODE_32_SELECTOR failed to init!")
        }
        AvailableSegmentSelector::UserData32 => {
            USER_DATA_32_SELECTOR.try().expect("USER_DATA_32_SELECTOR failed to init!")
        }
        AvailableSegmentSelector::UserCode64 => {
            USER_CODE_64_SELECTOR.try().expect("USER_CODE_32_SELECTOR failed to init!")
        }
        AvailableSegmentSelector::UserData64 => {
            USER_DATA_64_SELECTOR.try().expect("USER_DATA_32_SELECTOR failed to init!")
        }
        AvailableSegmentSelector::Tss => {
            TSS_SELECTOR.try().expect("TSS_SELECTOR failed to init!")
        }
    };

    SegmentSelector::new(seg.index(), seg.rpl())
}



/// Interface to our PIC (programmable interrupt controller) chips.
/// We want to map hardware interrupts to 0x20 (for PIC1) or 0x28 (for PIC2).
static PIC: Once<pic::ChainedPics> = Once::new();
static KEYBOARD: Mutex<Port<u8>> = Mutex::new(Port::new(0x60));

static TSS: Mutex<TaskStateSegment> = Mutex::new(TaskStateSegment::new());
static GDT: Once<gdt::Gdt> = Once::new();



/// Sets the TSS's privilege stack 0 (RSP0) entry, which points to the stack that 
/// the x86_64 hardware automatically switches to when transitioning from Ring 3 -> Ring 0.
/// Should be set to an address within the current userspace task's kernel stack.
/// WARNING: If set incorrectly, the OS will crash upon an interrupt from userspace into kernel space!!
pub fn tss_set_rsp0(new_value: usize) {
    use x86_64::VirtualAddress;
    if let Some(mut tss) = TSS.try_lock() {
        tss.privilege_stack_table[0] = VirtualAddress(new_value);
    }
    else {
        panic!("FATAL ERROR: TSS was locked in tss_set_rsp0!!");
    }
}



/// initializes the interrupt subsystem and exception-related IRQs, but no other IRQs.
/// Arguments: the address of the top of a newly allocated stack, to be used as the double fault exception handler stack 
/// Arguments: the address of the top of a newly allocated stack, to be used as the privilege stack (Ring 3 -> Ring 0 stack)
pub fn init(double_fault_stack_top_unusable: usize, privilege_stack_top_unusable: usize) {
    assert_has_not_been_called!("interrupts::init was called more than once!");
    
    use x86_64::instructions::segmentation::{set_cs, load_ds, load_ss};
    use x86_64::instructions::tables::load_tss;
    use x86_64::PrivilegeLevel;
    use x86_64::VirtualAddress;

    // set up TSS and get pointer to it    
    let tss_ptr: u64 = {
        let mut tss = TSS.lock();
        // TSS.RSP0 is used in kernel space after a transition from Ring 3 -> Ring 0
        tss.privilege_stack_table[0] = VirtualAddress(privilege_stack_top_unusable);
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX] = VirtualAddress(double_fault_stack_top_unusable);

        // get the pointer to the raw TSS structure inside the TSS mutex, required for x86's load tss instruction
        &*tss as *const _ as u64
    };
    

    let gdt = GDT.call_once(|| {
        let mut gdt = gdt::Gdt::new();

        // this order of code segments must be preserved: kernel cs, kernel ds, user cs 32, user ds 32, user cs 64, user ds 64, tss

        KERNEL_CODE_SELECTOR.call_once(|| {
            gdt.add_entry(gdt::Descriptor::kernel_code_segment(), PrivilegeLevel::Ring0)
        });
        KERNEL_DATA_SELECTOR.call_once(|| {
            gdt.add_entry(gdt::Descriptor::kernel_data_segment(), PrivilegeLevel::Ring0)
        });
        USER_CODE_32_SELECTOR.call_once(|| {
            gdt.add_entry(gdt::Descriptor::user_code_32_segment(), PrivilegeLevel::Ring3)
        });
        USER_DATA_32_SELECTOR.call_once(|| {
            gdt.add_entry(gdt::Descriptor::user_data_32_segment(), PrivilegeLevel::Ring3)
        });
        USER_CODE_64_SELECTOR.call_once(|| {
            gdt.add_entry(gdt::Descriptor::user_code_64_segment(), PrivilegeLevel::Ring3)
        });
        USER_DATA_64_SELECTOR.call_once(|| {
            gdt.add_entry(gdt::Descriptor::user_data_64_segment(), PrivilegeLevel::Ring3)
        });
        TSS_SELECTOR.call_once(|| {
            gdt.add_entry(gdt::Descriptor::tss_segment(tss_ptr), PrivilegeLevel::Ring0)
        });
        gdt
    });
    gdt.load();


    debug!("Loaded GDT: {}", gdt);

    unsafe {
        set_cs(get_segment_selector(AvailableSegmentSelector::KernelCode)); // reload code segment register
        load_tss(get_segment_selector(AvailableSegmentSelector::Tss)); // load TSS
        
        load_ss(get_segment_selector(AvailableSegmentSelector::KernelData)); // unsure if necessary
        load_ds(get_segment_selector(AvailableSegmentSelector::KernelData)); // unsure if necessary
    }


    {
        let mut idt = IDT.lock(); // withholds interrupts

        // SET UP FIXED EXCEPTION HANDLERS
        idt.divide_by_zero.set_handler_fn(divide_by_zero_handler);
        // missing: 0x01 debug exception
        // missing: 0x02 non-maskable interrupt exception
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        // missing: 0x04 overflow exception
        // missing: 0x05 bound range exceeded exception
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.device_not_available.set_handler_fn(device_not_available_handler);
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(DOUBLE_FAULT_IST_INDEX as u16); // use a special stack for the DF handler
        }
        // reserved: 0x09 coprocessor segment overrun exception
        // missing: 0x0a invalid TSS exception
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        // missing: 0x0c stack segment exception
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        // reserved: 0x0f vector 15
        // missing: 0x10 floating point exception
        // missing: 0x11 alignment check exception
        // missing: 0x12 machine check exception
        // missing: 0x13 SIMD floating point exception
        // missing: 0x14 virtualization vector 20
        // missing: 0x15 - 0x1d SIMD floating point exception
        // missing: 0x1e security exception
        // reserved: 0x1f

        // fill all IDT entries with an unimplemented IRQ handler
        for i in 32..255 {
            idt[i].set_handler_fn(unimplemented_interrupt_handler);
        }

    }

    
    {
        info!("trying to load IDT...");
        IDT.load();
        info!("loaded interrupt descriptor table.");
    }

}


pub fn init_handlers_apic() {
    // first, do the standard interrupt remapping, but mask all PIC interrupts / disable the PIC
    PIC.call_once( || {
        pic::ChainedPics::init(None, None, 0xFF, 0xFF) // disable all PIC IRQs
    });

    {
        let mut idt = IDT.lock(); // withholds interrupts

        // quick test to just try these two out, since the PIC is not using our static IDT
        idt[0x20].set_handler_fn(apic_timer_handler);
        idt[0x27].set_handler_fn(apic_spurious_interrupt_handler); 
        idt[apic::APIC_SPURIOUS_INTERRUPT_VECTOR as usize].set_handler_fn(apic_0xff_handler); 
    }
}


pub fn init_handlers_pic() {
    {
        let mut idt = IDT.lock(); // withholds interrupts
		// SET UP CUSTOM INTERRUPT HANDLERS
		// we can directly index the "idt" object because it implements the Index/IndexMut traits

        // MASTER PIC starts here (0x20 - 0x27)
        idt[0x20].set_handler_fn(timer_handler);
        idt[0x21].set_handler_fn(keyboard_handler);
        
        idt[0x22].set_handler_fn(irq_0x22_handler); 
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




/// interrupt 0x00
extern "x86-interrupt" fn divide_by_zero_handler(stack_frame: &mut ExceptionStackFrame) {
    println_unsafe!("\nEXCEPTION: DIVIDE BY ZERO\n{:#?}", stack_frame);
    loop {}
}

/// interrupt 0x03
extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut ExceptionStackFrame) {
    println_unsafe!("\nEXCEPTION: BREAKPOINT at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
}

/// interrupt 0x06
extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: &mut ExceptionStackFrame) {
    println_unsafe!("\nEXCEPTION: INVALID OPCODE at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
    loop {}
}

/// interrupt 0x07
/// see this: http://wiki.osdev.org/I_Cant_Get_Interrupts_Working#I_keep_getting_an_IRQ7_for_no_apparent_reason
extern "x86-interrupt" fn device_not_available_handler(stack_frame: &mut ExceptionStackFrame) {
    println_unsafe!("\nEXCEPTION: DEVICE_NOT_AVAILABLE at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);

    loop {}
}



extern "x86-interrupt" fn page_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: PageFaultErrorCode) {
    use x86_64::registers::control_regs;
    println_unsafe!("\nEXCEPTION: PAGE FAULT while accessing {:#x}\nerror code: \
                                  {:?}\n{:#?}",
             control_regs::cr2(),
             error_code,
             stack_frame);
    loop {}
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: &mut ExceptionStackFrame, _error_code: u64) {
    println_unsafe!("\nEXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
    loop {}
}



/// this shouldn't really ever happen, but I added the handler anyway
/// because I noticed the interrupt 0xb happening when other interrupts weren't properly handled
extern "x86-interrupt" fn segment_not_present_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    // use x86_64::registers::control_regs;
    println_unsafe!("\nEXCEPTION: SEGMENT_NOT_PRESENT FAULT\nerror code: \
                                  {:#b}\n{:#?}",
//             control_regs::cr2(),
             error_code,
             stack_frame);

    loop {}
}


extern "x86-interrupt" fn general_protection_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    println_unsafe!("\nEXCEPTION: GENERAL PROTECTION FAULT \nerror code: \
                                  {:#b}\n{:#?}",
             error_code,
             stack_frame);


    // TODO: kill the offending process
    loop {}
}



// 0x20
extern "x86-interrupt" fn apic_timer_handler(stack_frame: &mut ExceptionStackFrame) {
    info!("APIC TIMER HANDLER!");

    let mut lapic_locked = apic::get_lapic();
    let mut local_apic = lapic_locked.as_mut().expect("apic_timer_handler(): local_apic wasn't yet inited!");
    local_apic.eoi(0x20);
}


// 0x27
extern "x86-interrupt" fn apic_spurious_interrupt_handler(stack_frame: &mut ExceptionStackFrame) {
    info!("APIC SPURIOUS INTERRUPT HANDLER!");

    let mut lapic_locked = apic::get_lapic();
    let mut local_apic = lapic_locked.as_mut().expect("apic_spurious_interrupt_handler(): local_apic wasn't yet inited!");
    local_apic.eoi(0x27);
}

// 0x27
extern "x86-interrupt" fn apic_0xff_handler(stack_frame: &mut ExceptionStackFrame) {
    info!("APIC 0xFF HANDLER!");

    let mut lapic_locked = apic::get_lapic();
    let mut local_apic = lapic_locked.as_mut().expect("apic_0xff_handler(): local_apic wasn't yet inited!");
    local_apic.eoi(0xFF);
}


// 0x20
extern "x86-interrupt" fn timer_handler(stack_frame: &mut ExceptionStackFrame) {
    pit_clock::handle_timer_interrupt();

	PIC.try().expect("IRQ 0x20: PIC not initialized").notify_end_of_interrupt(0x20);
}


// 0x21
extern "x86-interrupt" fn keyboard_handler(stack_frame: &mut ExceptionStackFrame) {
    // in this interrupt, we must read the keyboard scancode register before acknowledging the interrupt.
    let scan_code: u8 = { 
        KEYBOARD.lock().read() 
    };
	// trace!("KBD: {:?}", scan_code);

    keyboard::handle_keyboard_input(scan_code);	

    PIC.try().expect("IRQ 0x21: PIC not initialized").notify_end_of_interrupt(0x21);
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
            pic.notify_end_of_interrupt(0x27);
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
    if let Some(ticks) = rtc_ticks {      
        if (ticks % (CONFIG_TIMESLICE_PERIOD_MS * CONFIG_RTC_FREQUENCY_HZ / 1000)) == 0 {
            schedule!();
        }
    }
    else {
        error!("RTC interrupt function: unable to get RTC_TICKS system-wide state.")
    }
}

// //0x28
// extern "x86-interrupt" fn rtc_handler(stack_frame: &mut ExceptionStackFrame ) {
//     // because we use the RTC interrupt handler for context switching,
//     // we must ack the interrupt and send EOI before calling the handler, 
//     // because the handler will not return.
//     rtc::rtc_ack_irq();
//     unsafe { PIC.notify_end_of_interrupt(0x28); }
    
//     rtc::handle_rtc_interrupt();
// }


//0x2e
extern "x86-interrupt" fn primary_ata(stack_frame:&mut ExceptionStackFrame ) {

    //let placeholder = 2;
    
    ata_pio::handle_primary_interrupt();

    PIC.try().expect("IRQ 0x21: PIC not initialized").notify_end_of_interrupt(0x2e);
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

