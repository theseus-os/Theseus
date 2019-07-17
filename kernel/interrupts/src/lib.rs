//! Basic interrupt handling structures and simple handler routines.

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
extern crate exceptions_early;
extern crate pic;
extern crate scheduler;
extern crate keyboard;
extern crate mouse;
extern crate ps2;
extern crate tlb_shootdown;



use ps2::handle_mouse_packet;
use x86_64::structures::idt::{Idt, LockedIdt, ExceptionStackFrame, HandlerFunc};
use spin::Once;
use kernel_config::time::{CONFIG_PIT_FREQUENCY_HZ}; //, CONFIG_RTC_FREQUENCY_HZ};
// use rtc;
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use memory::VirtualAddress;
use apic::{INTERRUPT_CHIP, InterruptChip};
use pic::PIC_MASTER_OFFSET;


/// The single system-wide IDT
/// Note: this could be per-core instead of system-wide, if needed.
pub static IDT: LockedIdt = LockedIdt::new();

/// The single system-wide Programmable Interrupt Controller (PIC) chip.
static PIC: Once<pic::ChainedPics> = Once::new();



/// initializes the interrupt subsystem and properly sets up safer early exception handlers, but no other IRQ handlers.
/// # Arguments: 
/// * `double_fault_stack_top_unusable`: the address of the top of a newly allocated stack, to be used as the double fault exception handler stack.
/// * `privilege_stack_top_unusable`: the address of the top of a newly allocated stack, to be used as the privilege stack (Ring 3 -> Ring 0 stack).
pub fn init(double_fault_stack_top_unusable: VirtualAddress, privilege_stack_top_unusable: VirtualAddress) 
    -> Result<&'static LockedIdt, &'static str> 
{
    let bsp_id = try!(apic::get_bsp_id().ok_or("couldn't get BSP's id"));
    info!("Setting up TSS & GDT for BSP (id {})", bsp_id);
    gdt::create_tss_gdt(bsp_id, double_fault_stack_top_unusable, privilege_stack_top_unusable);

    // initialize early exception handlers
    exceptions_early::init(&IDT);
    #[cfg(any(target_arch="x86", target_arch="x86_64"))]
    {
        // set the special double fault handler's stack
        let mut idt = IDT.lock(); // withholds interrupts
        unsafe {
            // use a special stack for the double fault handler, which prevents triple faults!
            idt.double_fault.set_handler_fn(exceptions_early::double_fault_handler)
                            .set_stack_index(tss::DOUBLE_FAULT_IST_INDEX as u16); 
        }
       
        // fill all IDT entries with an unimplemented IRQ handler
        for i in 32..255 {
            idt[i].set_handler_fn(unimplemented_interrupt_handler);
        }
    }

    // try to load our new IDT    
    {
        info!("trying to load IDT for BSP...");
        IDT.load();
        info!("loaded IDT for BSP.");
    }

    Ok(&IDT)

}


/// Similar to `init()`, but for APs to call after the BSP has already invoked `init()`.
pub fn init_ap(apic_id: u8, 
               double_fault_stack_top_unusable: VirtualAddress, 
               privilege_stack_top_unusable: VirtualAddress)
               -> Result<&'static LockedIdt, &'static str> {
    info!("Setting up TSS & GDT for AP {}", apic_id);
    gdt::create_tss_gdt(apic_id, double_fault_stack_top_unusable, privilege_stack_top_unusable);

    // We've already created the IDT initially (currently all APs share the BSP's IDT),
    // so we only need to re-load it here for each AP.
    IDT.load();
    info!("loaded IDT for AP {}.", apic_id);
    Ok(&IDT)
}


/// Establishes the default interrupt handlers that are statically known.
fn set_handlers(idt: &mut Idt) {
    // exceptions (IRQS from 0-31) have already been inited before

    // fill all IDT entries with an unimplemented IRQ handler
    #[cfg(any(target_arch="x86", target_arch="x86_64"))]
    {
        for i in 32..255 {
            idt[i].set_handler_fn(unimplemented_interrupt_handler);
        }

        idt[0x20].set_handler_fn(pit_timer_handler);
        idt[0x21].set_handler_fn(ps2_keyboard_handler);
        idt[0x22].set_handler_fn(lapic_timer_handler);
        idt[0x23].set_handler_fn(unimplemented_interrupt_handler);
        idt[0x24].set_handler_fn(com1_serial_handler);
        idt[0x25].set_handler_fn(unimplemented_interrupt_handler);
        idt[0x26].set_handler_fn(unimplemented_interrupt_handler);
        idt[0x27].set_handler_fn(pic_spurious_interrupt_handler); 

        // idt[0x28].set_handler_fn(rtc_handler);
        idt[0x28].set_handler_fn(unimplemented_interrupt_handler);
        idt[0x29].set_handler_fn(unimplemented_interrupt_handler);
        idt[0x2A].set_handler_fn(unimplemented_interrupt_handler);
        idt[0x2B].set_handler_fn(unimplemented_interrupt_handler);
        idt[0x2C].set_handler_fn(ps2_mouse_handler);
        idt[0x2D].set_handler_fn(unimplemented_interrupt_handler);
        idt[0x2E].set_handler_fn(primary_ata_handler);
        idt[0x2F].set_handler_fn(secondary_ata_handler);

        idt[apic::APIC_SPURIOUS_INTERRUPT_VECTOR as usize].set_handler_fn(apic_spurious_interrupt_handler); 
        idt[tlb_shootdown::TLB_SHOOTDOWN_IPI_IRQ as usize].set_handler_fn(ipi_handler);
    }
}


pub fn init_handlers_apic() {
    // first, do the standard interrupt remapping, but mask all PIC interrupts / disable the PIC
    PIC.call_once(|| pic::ChainedPics::init(0xFF, 0xFF)); // disable all PIC IRQs
    
    set_handlers(&mut IDT.lock());
}


pub fn init_handlers_pic() {
    set_handlers(&mut IDT.lock());

    // init PIC, PIT and RTC interrupts
    let master_pic_mask: u8 = 0x0; // allow every interrupt
    let slave_pic_mask: u8 = 0b0000_1000; // everything is allowed except 0x2B 
    PIC.call_once(|| pic::ChainedPics::init(master_pic_mask, slave_pic_mask));

    pit_clock::init(CONFIG_PIT_FREQUENCY_HZ);
    // let rtc_handler = rtc::init(CONFIG_RTC_FREQUENCY_HZ, rtc_interrupt_func);
    // IDT.lock()[0x28].set_handler_fn(rtc_handler.unwrap());
}

/// Registers an interrupt handler. 
/// The function fails if the interrupt number is already in use. 
/// 
/// # Arguments 
/// * `interrupt_num` - the interrupt that is being requested
/// * `func` - the handler to be registered for 'interrupt_num'
pub fn register_interrupt(interrupt_num: u8, func: HandlerFunc) -> Result<(), &'static str> {
    let mut idt = IDT.lock();

    // checks if the handler stored is the default apic handler which signifies that the interrupt hasn't been used yet
    #[cfg(any(target_arch="x86", target_arch="x86_64"))]
    {
        if idt[interrupt_num as usize].handler_eq(unimplemented_interrupt_handler) {
            idt[interrupt_num as usize].set_handler_fn(func);
            Ok(())
        }
        else {
            error!("register_interrupt: the requested interrupt IRQ {} is not available", interrupt_num);
            Err("register_interrupt: the requested interrupt is not available")
        }
    }

    #[cfg(any(target_arch="aarch64"))]
    Ok(())
} 

/// Returns an interrupt number assigned by the OS and sets its handler function. 
/// The function fails if there is no unused interrupt number.
/// 
/// # Arguments
/// * `func` - the handler for the assigned interrupt number
pub fn register_msi_interrupt(func: HandlerFunc) -> Result<u8, &'static str> {
    let mut idt = IDT.lock();

    // try to find an unused interrupt 
    #[cfg(any(target_arch="x86", target_arch="x86_64"))]
    {
        let interrupt_num = (*idt).find_free_entry(unimplemented_interrupt_handler).ok_or("register_msi_interrupt: no available interrupt")?;
        idt[interrupt_num].set_handler_fn(func);
    
        return Ok(interrupt_num as u8)
    }

    #[cfg(any(target_arch="aarch64"))]
    Ok(0)
} 

/// Returns an interrupt to the system by setting the handler to the default function. 
/// The application provides the current interrupt handler as a safety check. 
/// The function fails if the current handler and 'func' do not match
/// 
/// # Arguments
/// * `interrupt_num` - the interrupt that needs to be deregistered
/// * `func` - the handler that should currently be stored for 'interrupt_num'
pub fn deregister_interrupt(interrupt_num: u8, func: HandlerFunc) -> Result<(), &'static str> {
    let mut idt = IDT.lock();

    // check if the handler stored is the same as the one provided
    // this is to make sure no other application can deregister your interrupt
    if idt[interrupt_num as usize].handler_eq(func) {
        #[cfg(any(target_arch="x86", target_arch="x86_64"))]
        idt[interrupt_num as usize].set_handler_fn(unimplemented_interrupt_handler);
        Ok(())
    }
    else {
        error!("deregister_interrupt: Cannot free interrupt due to incorrect handler function");
        Err("deregister_interrupt: Cannot free interrupt due to incorrect handler function")
    }
}

/// Send an end of interrupt signal, which works for all types of interrupt chips (APIC, x2apic, PIC)
/// irq arg is only used for PIC
pub fn eoi(irq: Option<u8>) {
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
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn pit_timer_handler(_stack_frame: &mut ExceptionStackFrame) {
    pit_clock::handle_timer_interrupt();

	eoi(Some(PIC_MASTER_OFFSET));
}


// see this: https://forum.osdev.org/viewtopic.php?f=1&t=32655
static EXTENDED_SCANCODE: AtomicBool = AtomicBool::new(false);

/// 0x21
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn ps2_keyboard_handler(_stack_frame: &mut ExceptionStackFrame) {

    let indicator = ps2::ps2_status_register();

    // whether there is any data on the port 0x60
    if indicator & 0x01 == 0x01 {
        //whether the data is coming from the mouse
        if indicator & 0x20 != 0x20 {
            // in this interrupt, we must read the PS2_PORT scancode register before acknowledging the interrupt.
            let scan_code = ps2::ps2_read_data();
            // trace!("PS2_PORT interrupt: raw scan_code {:#X}", scan_code);


            let extended = EXTENDED_SCANCODE.load(Ordering::SeqCst);

            // 0xE0 indicates an extended scancode, so we must wait for the next interrupt to get the actual scancode
            if scan_code == 0xE0 {
                if extended {
                    error!("PS2_PORT interrupt: got two extended scancodes (0xE0) in a row! Shouldn't happen.");
                }
                // mark it true for the next interrupt
                EXTENDED_SCANCODE.store(true, Ordering::SeqCst);
            } else if scan_code == 0xE1 {
                error!("PAUSE/BREAK key pressed ... ignoring it!");
                // TODO: handle this, it's a 6-byte sequence (over the next 5 interrupts)
                EXTENDED_SCANCODE.store(true, Ordering::SeqCst);
            } else { // a regular scancode, go ahead and handle it
                // if the previous interrupt's scan_code was an extended scan_code, then this one is not
                if extended {
                    EXTENDED_SCANCODE.store(false, Ordering::SeqCst);
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
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn ps2_mouse_handler(_stack_frame: &mut ExceptionStackFrame) {

    let indicator = ps2::ps2_status_register();

    // whether there is any data on the port 0x60
    if indicator & 0x01 == 0x01 {
        //whether the data is coming from the mouse
        if indicator & 0x20 == 0x20 {
            let readdata = handle_mouse_packet();
            if (readdata & 0x80 == 0x80) || (readdata & 0x40 == 0x40) {
                error!("The overflow bits in the mouse data packet's first byte are set! Discarding the whole packet.");
            } else if readdata & 0x08 == 0 {
                error!("Third bit should in the mouse data packet's first byte should be always be 1. Discarding the whole packet since the bit is 0 now.");
            } else {
                let _mouse_event = mouse::handle_mouse_input(readdata);
                // mouse::mouse_to_print(&_mouse_event);
            }

        }

    }

    eoi(Some(PIC_MASTER_OFFSET + 0xc));
}

pub static APIC_TIMER_TICKS: AtomicUsize = AtomicUsize::new(0);
/// 0x22
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn lapic_timer_handler(_stack_frame: &mut ExceptionStackFrame) {
    let _ticks = APIC_TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    // info!(" ({}) APIC TIMER HANDLER! TICKS = {}", apic::get_my_apic_id().unwrap_or(0xFF), _ticks);
    
    // we must acknowledge the interrupt first before handling it because we switch tasks here, which doesn't return
    eoi(None); // None, because 0x22 IRQ cannot possibly be a PIC interrupt
    
    scheduler::schedule();
}


/// 0x24
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn com1_serial_handler(_stack_frame: &mut ExceptionStackFrame) {
    info!("COM1 serial handler");

    eoi(Some(PIC_MASTER_OFFSET + 0x4));
}

#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn apic_spurious_interrupt_handler(_stack_frame: &mut ExceptionStackFrame) {
    warn!("APIC SPURIOUS INTERRUPT HANDLER!");

    eoi(None);
}

#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn unimplemented_interrupt_handler(_stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nUnimplemented interrupt handler: {:#?}", _stack_frame);
	match apic::INTERRUPT_CHIP.load(Ordering::Acquire) {
        apic::InterruptChip::PIC => {
            let irq_regs = PIC.try().map(|pic| pic.read_isr_irr());  
            println_raw!("PIC IRQ Registers: {:?}", irq_regs);
        }
        apic::InterruptChip::APIC | apic::InterruptChip::X2APIC => {
            if let Some(lapic_ref) = apic::get_my_apic() {
                let lapic = lapic_ref.read();
                let isr = lapic.get_isr(); 
                let irr = lapic.get_irr();
                println_raw!("APIC ISR: {:#x} {:#x} {:#x} {:#x}, {:#x} {:#x} {:#x} {:#x}\n \
                    IRR: {:#x} {:#x} {:#x} {:#x},{:#x} {:#x} {:#x} {:#x}", 
                    isr[0], isr[1], isr[2], isr[3], isr[4], isr[5], isr[6], isr[7],
                    irr[0], irr[1], irr[2], irr[3], irr[4], irr[5], irr[6], irr[7],
                );
            }
            else {
                println_raw!("APIC ISR and IRR were unknown.");
            }
        }
    };

    loop { }
}



/// The Spurious interrupt handler for the PIC. 
/// This has given us a lot of problems on bochs emulator and on some real hardware, but not on QEMU.
/// Spurious interrupts occur a lot when using PIC on real hardware, but only occurs once when using apic/x2apic. 
/// See here for more: https://mailman.linuxchix.org/pipermail/techtalk/2002-August/012697.html.
/// We handle it according to this advice: https://wiki.osdev.org/8259_PIC#Spurious_IRQs
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn pic_spurious_interrupt_handler(_stack_frame: &mut ExceptionStackFrame ) {
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
        error!("pic_spurious_interrupt_handler(): PIC wasn't initialized!");
    }

}



// fn rtc_interrupt_func(rtc_ticks: Option<usize>) {
//     trace!("rtc_interrupt_func: rtc_ticks = {:?}", rtc_ticks);
// }

// //0x28
// extern "x86-interrupt" fn rtc_handler(_stack_frame: &mut ExceptionStackFrame ) {
//     // because we use the RTC interrupt handler for task switching,
//     // we must ack the interrupt and send EOI before calling the handler, 
//     // because the handler will not return.
//     rtc::rtc_ack_irq();
//     eoi(Some(PIC_MASTER_OFFSET + 0x8));
    
//     rtc::handle_rtc_interrupt();
// }


/// 0x2E
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn primary_ata_handler(_stack_frame: &mut ExceptionStackFrame ) {
    info!("Primary ATA Interrupt (0x2E)");

    eoi(Some(PIC_MASTER_OFFSET + 0xE));
}


/// 0x2F
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn secondary_ata_handler(_stack_frame: &mut ExceptionStackFrame ) {
    info!("Secondary ATA Interrupt (0x2F)");
    
    eoi(Some(PIC_MASTER_OFFSET + 0xF));
}


#[cfg(any(target_arch="x86", target_arch="x86_64"))]
extern "x86-interrupt" fn ipi_handler(_stack_frame: &mut ExceptionStackFrame) {
    eoi(None);
}
