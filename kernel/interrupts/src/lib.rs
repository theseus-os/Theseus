// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
#![no_std]
#![feature(abi_x86_interrupt)]

#![allow(dead_code)]


#[macro_use] extern crate log;
#[macro_use] extern crate vga_buffer;
extern crate x86_64;
extern crate spin;
extern crate port_io;
extern crate kernel_config;
extern crate memory;
extern crate apic;
extern crate pit_clock;
extern crate tss;
extern crate gdt;
extern crate exceptions;
extern crate pic;
extern crate scheduler;
extern crate keyboard;
extern crate mouse;
extern crate ps2;



use ps2::handle_mouse_packet;
use mouse::mouse_to_print;
use x86_64::structures::idt::{LockedIdt, ExceptionStackFrame};
use spin::Once;
//use port_io::Port;
// use drivers::ata_pio;
use kernel_config::time::{CONFIG_PIT_FREQUENCY_HZ}; //, CONFIG_RTC_FREQUENCY_HZ};
// use rtc;
use core::sync::atomic::{AtomicUsize, Ordering};
use memory::VirtualAddress;
use apic::{INTERRUPT_CHIP, InterruptChip};
use pic::PIC_MASTER_OFFSET;

// use drivers::e1000;


/// The single system-wide IDT
/// Note: this could be per-core instead of system-wide, if needed.
pub static IDT: LockedIdt = LockedIdt::new();

/// Interface to our PIC (programmable interrupt controller) chips.
static PIC: Once<pic::ChainedPics> = Once::new();



/// initializes the interrupt subsystem and properly sets up safer exception-related IRQs, but no other IRQ handlers.
/// Arguments: the address of the top of a newly allocated stack, to be used as the double fault exception handler stack 
/// Arguments: the address of the top of a newly allocated stack, to be used as the privilege stack (Ring 3 -> Ring 0 stack)
pub fn init(double_fault_stack_top_unusable: VirtualAddress, privilege_stack_top_unusable: VirtualAddress) -> Result<(), &'static str> {

    let bsp_id = try!(apic::get_bsp_id().ok_or("couldn't get BSP's id"));
    info!("Setting up TSS & GDT for BSP (id {})", bsp_id);
    gdt::create_tss_gdt(bsp_id, double_fault_stack_top_unusable, privilege_stack_top_unusable);

    {
        let mut idt = IDT.lock(); // withholds interrupts
        
        idt.divide_by_zero.set_handler_fn(exceptions::divide_by_zero_handler);
        // missing: 0x01 debug exception
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler); // use our local NMI handler, not the default one in exceptions
        idt.breakpoint.set_handler_fn(exceptions::breakpoint_handler);
        // missing: 0x04 overflow exception
        // missing: 0x05 bound range exceeded exception
        idt.invalid_opcode.set_handler_fn(exceptions::invalid_opcode_handler);
        idt.device_not_available.set_handler_fn(exceptions::device_not_available_handler);
        unsafe {
            // use a special stack for the double fault handler
            idt.double_fault.set_handler_fn(exceptions::double_fault_handler)
                            .set_stack_index(tss::DOUBLE_FAULT_IST_INDEX as u16); 
        }
        // reserved: 0x09 coprocessor segment overrun exception
        // missing: 0x0a invalid TSS exception
        idt.segment_not_present.set_handler_fn(exceptions::segment_not_present_handler);
        // missing: 0x0c stack segment exception
        idt.general_protection_fault.set_handler_fn(exceptions::general_protection_fault_handler);
        idt.page_fault.set_handler_fn(exceptions::page_fault_handler);
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
            idt[i].set_handler_fn(apic_unimplemented_interrupt_handler);
        }
    }

    // try to load our new IDT    
    {
        info!("trying to load IDT for BSP...");
        IDT.load();
        info!("loaded IDT for BSP.");
    }

    Ok(())

}


pub fn init_ap(apic_id: u8, 
               double_fault_stack_top_unusable: VirtualAddress, 
               privilege_stack_top_unusable: VirtualAddress)
               -> Result<(), &'static str> {
    info!("Setting up TSS & GDT for AP {}", apic_id);
    gdt::create_tss_gdt(apic_id, double_fault_stack_top_unusable, privilege_stack_top_unusable);

    // info!("trying to load IDT for AP {}...", apic_id);
    IDT.load();
    info!("loaded IDT for AP {}.", apic_id);
    Ok(())
}


pub fn init_handlers_apic() {
    // first, do the standard interrupt remapping, but mask all PIC interrupts / disable the PIC
    PIC.call_once( || {
        pic::ChainedPics::init(0xFF, 0xFF) // disable all PIC IRQs
    });

    {
        let mut idt = IDT.lock(); // withholds interrupts
        
        // exceptions (IRQS from 0-31) have already been inited before

        // fill all IDT entries with an unimplemented IRQ handler
        for i in 32..255 {
            idt[i].set_handler_fn(apic_unimplemented_interrupt_handler);
        }
        
        idt[0x20].set_handler_fn(pit_timer_handler);
        idt[0x21].set_handler_fn(ps2_keyboard_handler);
        idt[0x22].set_handler_fn(lapic_timer_handler);
        idt[0x24].set_handler_fn(com1_serial_handler);
        // idt[0x25].set_handler_fn(irq_0x25_handler);
        idt[0x26].set_handler_fn(apic_irq_0x26_handler);
        idt[0x27].set_handler_fn(spurious_interrupt_handler); 

        // idt[0x28].set_handler_fn(irq_0x28_handler);
        idt[0x29].set_handler_fn(nic_handler); // for Bochs
        // idt[0x2A].set_handler_fn(irq_0x2A_handler);
        idt[0x2B].set_handler_fn(nic_handler);
        idt[0x2C].set_handler_fn(ps2_mouse_handler);
        // idt[0x2D].set_handler_fn(irq_0x2D_handler);
        // idt[0x2E].set_handler_fn(irq_0x2E_handler);
        // idt[0x2F].set_handler_fn(irq_0x2F_handler);

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
        idt[0x21].set_handler_fn(ps2_keyboard_handler);
        // there is no IRQ 0x22        
        idt[0x23].set_handler_fn(irq_0x23_handler); 
        idt[0x24].set_handler_fn(com1_serial_handler); 
        idt[0x25].set_handler_fn(irq_0x25_handler); 
        idt[0x26].set_handler_fn(irq_0x26_handler); 

        idt[0x27].set_handler_fn(spurious_interrupt_handler); 


        // SLAVE PIC starts here (0x28 - 0x2E)        
        // idt[0x28].set_handler_fn(rtc_handler); // using the weird way temporarily

        idt[0x29].set_handler_fn(irq_0x29_handler); 
        idt[0x2A].set_handler_fn(irq_0x2A_handler); 
        //idt[0x2B].set_handler_fn(irq_0x2B_handler);
        idt[0x2B].set_handler_fn(nic_handler); 
        idt[0x2C].set_handler_fn(ps2_mouse_handler);
        idt[0x2D].set_handler_fn(irq_0x2D_handler); 

        idt[0x2E].set_handler_fn(primary_ata);
        // 0x2F missing right now

    }

    // init PIC, PIT and RTC interrupts
    let master_pic_mask: u8 = 0x0; // allow every interrupt
    let slave_pic_mask: u8 = 0b0000_1000; // everything is allowed except 0x2B 
    PIC.call_once( || {
        pic::ChainedPics::init(master_pic_mask, slave_pic_mask) // disable all PIC IRQs
    });

    pit_clock::init(CONFIG_PIT_FREQUENCY_HZ);
    // let rtc_handler = rtc::init(CONFIG_RTC_FREQUENCY_HZ, rtc_interrupt_func);
    // IDT.lock()[0x28].set_handler_fn(rtc_handler.unwrap());
}






/// Send an end of interrupt signal, which works for all types of interrupt chips (APIC, x2apic, PIC)
/// irq arg is only used for PIC
fn eoi(irq: Option<u8>) {
    match INTERRUPT_CHIP.load(Ordering::Acquire) {
        InterruptChip::APIC |
        InterruptChip::X2APIC => {
            apic::get_my_apic().expect("eoi(): couldn't get my apic to send EOI!").write().eoi();
        }
        InterruptChip::PIC => {
            PIC.try().expect("eoi(): PIC not initialized").notify_end_of_interrupt(irq.expect("PIC eoi, but no arg provided"));
        }
    }
}


/// 0x20
extern "x86-interrupt" fn pit_timer_handler(_stack_frame: &mut ExceptionStackFrame) {
    pit_clock::handle_timer_interrupt();

	eoi(Some(PIC_MASTER_OFFSET));
}


// see this: https://forum.osdev.org/viewtopic.php?f=1&t=32655
static mut EXTENDED_SCANCODE: bool = false;

/// 0x21
extern "x86-interrupt" fn ps2_keyboard_handler(_stack_frame: &mut ExceptionStackFrame) {
    let indicator = ps2::ps2_status_register();


    // whether there is any data on the port 0x60
    if indicator & 0x01 == 0x01 {
        //whether the data is coming from the mouse
        if indicator & 0x20 != 0x20 {
            // in this interrupt, we must read the PS2_PORT scancode register before acknowledging the interrupt.
            let scan_code = ps2::ps2_read_data();
            // trace!("PS2_PORT interrupt: raw scan_code {:#X}", scan_code);
//            ps2::disable_scanning();
//            ps2::enable_scanning();

            let extended = unsafe { EXTENDED_SCANCODE };

            // 0xE0 indicates an extended scancode, so we must wait for the next interrupt to get the actual scancode
            if scan_code == 0xE0 {
                if extended {
                    error!("PS2_PORT interrupt: got two extended scancodes (0xE0) in a row! Shouldn't happen.");
                }
                // mark it true for the next interrupt
                unsafe { EXTENDED_SCANCODE = true; }
            } else if scan_code == 0xE1 {
                error!("PAUSE/BREAK key pressed ... ignoring it!");
                // TODO: handle this, it's a 6-byte sequence (over the next 5 interrupts)
                unsafe { EXTENDED_SCANCODE = true; }
            } else { // a regular scancode, go ahead and handle it
                // if the previous interrupt's scan_code was an extended scan_code, then this one is not
                if extended {
                    unsafe { EXTENDED_SCANCODE = false; }
                }
                if scan_code != 0 {  // a scan code of zero is a PS2_PORT error that we can ignore
                    if let Err(e) = keyboard::handle_keyboard_input(scan_code, extended) {
                        error!("ps2_keyboard_handler: error handling PS2_PORT input: {:?}", e);
                    }
                }
            }
        }
    }
            eoi(Some(PIC_MASTER_OFFSET + 0x1));


}

/// 0x2C
#[allow(non_snake_case)]
extern "x86-interrupt" fn ps2_mouse_handler(_stack_frame: &mut ExceptionStackFrame) {
    let indicator = ps2::ps2_status_register();


    // whether there is any data on the port 0x60
    if indicator & 0x01 == 0x01 {
        //whether the data is coming from the mouse
        if indicator & 0x20 == 0x20 {
            let readdata = handle_mouse_packet();
            if (readdata & 0x80 == 0x80) || (readdata & 0x40 == 0x40) {
                error!("Displacement overflows!")
            } else if readdata & 0x08 == 0 {
                error!("third bit should always be 1")
            } else {
                let mouse_event = &mouse::handle_mouse_input(readdata);
                mouse_to_print(mouse_event);
            }

        }

    }

    eoi(Some(PIC_MASTER_OFFSET + 0xc));
}

pub static APIC_TIMER_TICKS: AtomicUsize = AtomicUsize::new(0);
/// 0x22
extern "x86-interrupt" fn lapic_timer_handler(_stack_frame: &mut ExceptionStackFrame) {
    let _ticks = APIC_TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    // info!(" ({}) APIC TIMER HANDLER! TICKS = {}", apic::get_my_apic_id().unwrap_or(0xFF), _ticks);
    
    eoi(None); // None, because it cannot possibly be a PIC interrupt
    // we must acknowledge the interrupt first before handling it because we context switch here, which doesn't return
    
    scheduler::schedule();
}


/// 0x24
extern "x86-interrupt" fn com1_serial_handler(_stack_frame: &mut ExceptionStackFrame) {
    // info!("COM1 serial handler");

    unsafe {
        x86_64::instructions::port::inb(0x3F8); // read serial port value
    }

    eoi(Some(PIC_MASTER_OFFSET + 0x4));
}

/// 0x26
extern "x86-interrupt" fn apic_irq_0x26_handler(_stack_frame: &mut ExceptionStackFrame) {
    // info!("APIX 0x26 IRQ handler");

    // unsafe {
    //     x86_64::instructions::port::inb(0x3F8); // read serial port value
    // }

    eoi(Some(PIC_MASTER_OFFSET + 0x6));
}


/// 0x2B
extern "x86-interrupt" fn nic_handler(_stack_frame: &mut ExceptionStackFrame) {
    debug!("nic handler called");
    e1000::e1000_handler();
	eoi(Some(0x2B));
}


extern "x86-interrupt" fn apic_spurious_interrupt_handler(_stack_frame: &mut ExceptionStackFrame) {
    warn!("APIC SPURIOUS INTERRUPT HANDLER!");

    eoi(None);
}

extern "x86-interrupt" fn apic_unimplemented_interrupt_handler(_stack_frame: &mut ExceptionStackFrame) {
    println_raw!("APIC UNIMPLEMENTED IRQ!!!");

    if let Some(lapic_ref) = apic::get_my_apic() {
        let lapic = lapic_ref.read();
        let isr = lapic.get_isr(); 
        let irr = lapic.get_irr();
        println_raw!("APIC ISR: {:#x} {:#x} {:#x} {:#x}, {:#x} {:#x} {:#x} {:#x} \nIRR: {:#x} {:#x} {:#x} {:#x},{:#x} {:#x} {:#x} {:#x}", 
                         isr.0, isr.1, isr.2, isr.3, isr.4, isr.5, isr.6, isr.7, irr.0, irr.1, irr.2, irr.3, irr.4, irr.5, irr.6, irr.7);
    }
    else {
        println_raw!("apic_unimplemented_interrupt_handler: couldn't get my apic.");
    }

    loop { }

    // eoi(None);
}



pub static mut SPURIOUS_COUNT: u64 = 0;

/// The Spurious interrupt handler. 
/// This has given us a lot of problems on bochs emulator and on some real hardware, but not on QEMU.
/// Spurious interrupts occur a lot when using PIC on real hardware, but only occurs once when using apic/x2apic. 
/// See here for more: https://mailman.linuxchix.org/pipermail/techtalk/2002-August/012697.html.
/// We handle it according to this advice: https://wiki.osdev.org/8259_PIC#Spurious_IRQs
extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: &mut ExceptionStackFrame ) {
    unsafe { SPURIOUS_COUNT += 1; } // cheap counter just for debug info

    if let Some(pic) = PIC.try() {
        let irq_regs = pic.read_isr_irr();
        // check if this was a real IRQ7 (parallel port) (bit 7 will be set)
        // (pretty sure this will never happen)
        // if it was a real IRQ7, we do need to ack it by sending an EOI
        if irq_regs.master_isr & 0x80 == 0x80 {
            println_raw!("\nGot real IRQ7, not spurious! (Unexpected behavior)");
            error!("Got real IRQ7, not spurious! (Unexpected behavior)");
            eoi(Some(PIC_MASTER_OFFSET + 0x7));
        }
        else {
            // do nothing. Do not send an EOI. 
            // see https://wiki.osdev.org/8259_PIC#Spurious_IRQs
        }
    }
    else {
        error!("spurious_interrupt_handler(): PIC wasn't initialized!");
    }

}



// fn rtc_interrupt_func(rtc_ticks: Option<usize>) {
//     trace!("rtc_interrupt_func: rtc_ticks = {:?}", rtc_ticks);
// }

// //0x28
// extern "x86-interrupt" fn rtc_handler(_stack_frame: &mut ExceptionStackFrame ) {
//     // because we use the RTC interrupt handler for context switching,
//     // we must ack the interrupt and send EOI before calling the handler, 
//     // because the handler will not return.
//     rtc::rtc_ack_irq();
//     eoi(Some(PIC_MASTER_OFFSET + 0x8));
    
//     rtc::handle_rtc_interrupt();
// }


//0x2e
extern "x86-interrupt" fn primary_ata(_stack_frame:&mut ExceptionStackFrame ) {

    // ata_pio::handle_primary_interrupt();

    eoi(Some(PIC_MASTER_OFFSET + 0xe));
}


extern "x86-interrupt" fn unimplemented_interrupt_handler(_stack_frame: &mut ExceptionStackFrame) {
    let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());    
    println_raw!("UNIMPLEMENTED IRQ!!! {:?}", irq_regs);

    loop { }
}


extern "x86-interrupt" fn irq_0x22_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());    
    println_raw!("\nCaught 0x22 interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}

extern "x86-interrupt" fn irq_0x23_handler(_stack_frame: &mut ExceptionStackFrame) {
    let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
	println_raw!("\nCaught 0x23 interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}

extern "x86-interrupt" fn irq_0x24_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());
    println_raw!("\nCaught 0x24 interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}

extern "x86-interrupt" fn irq_0x25_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_raw!("\nCaught 0x25 interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}


extern "x86-interrupt" fn irq_0x26_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_raw!("\nCaught 0x26 interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}

extern "x86-interrupt" fn irq_0x27_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_raw!("\nCaught 0x27 interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}


extern "x86-interrupt" fn irq_0x28_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_raw!("\nCaught 0x28 interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}


extern "x86-interrupt" fn irq_0x29_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_raw!("\nCaught 0x29 interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}


#[allow(non_snake_case)]
extern "x86-interrupt" fn irq_0x2A_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_raw!("\nCaught 0x2A interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}

#[allow(non_snake_case)]
extern "x86-interrupt" fn irq_0x2B_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_raw!("\nCaught 0x2B interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}



#[allow(non_snake_case)]
extern "x86-interrupt" fn irq_0x2D_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_raw!("\nCaught 0x2D interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}

#[allow(non_snake_case)]
extern "x86-interrupt" fn irq_0x2E_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_raw!("\nCaught 0x2E interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}

#[allow(non_snake_case)]
extern "x86-interrupt" fn irq_0x2F_handler(_stack_frame: &mut ExceptionStackFrame) {
	let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
    println_raw!("\nCaught 0x2F interrupt: {:#?}", _stack_frame);
    println_raw!("IrqRegs: {:?}", irq_regs);

    loop { }
}



extern "x86-interrupt" fn ipi_handler(_stack_frame: &mut ExceptionStackFrame) {
    // Currently, IPIs are only used for TLB shootdowns.
    
    // trace!("ipi_handler (AP {})", apic::get_my_apic_id().unwrap_or(0xFF));
    apic::handle_tlb_shootdown_ipi();

    eoi(None);
}




extern "x86-interrupt" fn nmi_handler(stack_frame: &mut ExceptionStackFrame) {
    // currently we're using NMIs to send TLB shootdown IPIs
    let vaddr = apic::TLB_SHOOTDOWN_IPI_VIRT_ADDR.load(Ordering::Acquire);
    if vaddr != 0 {
        // trace!("nmi_handler (AP {})", apic::get_my_apic_id().unwrap_or(0xFF));
        apic::handle_tlb_shootdown_ipi(vaddr);
        return;
    }
    
    // if vaddr is 0, then it's a regular NMI    
    println_raw!("\nEXCEPTION: NON-MASKABLE INTERRUPT at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
    
    loop { }
}