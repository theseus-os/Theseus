//! Basic interrupt handling structures and simple handler routines.

#![no_std]
#![feature(abi_x86_interrupt)]

#![allow(dead_code)]

pub use pic::IRQ_BASE_OFFSET;

use ps2::handle_mouse_packet;
use x86_64::structures::idt::{InterruptStackFrame, HandlerFunc, InterruptDescriptorTable};
use spin::Once;
use kernel_config::time::CONFIG_PIT_FREQUENCY_HZ; //, CONFIG_RTC_FREQUENCY_HZ};
// use rtc;
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use memory::VirtualAddress;
use apic::{INTERRUPT_CHIP, InterruptChip};
use locked_idt::LockedIdt;
use log::{error, warn, info, debug, trace};
use vga_buffer::{print_raw, println_raw};


/// The single system-wide Interrupt Descriptor Table (IDT).
///
/// Note: this could be per-core instead of system-wide, if needed.
pub static IDT: LockedIdt = LockedIdt::new();

/// The single system-wide Programmable Interrupt Controller (PIC) chip.
static PIC: Once<pic::ChainedPics> = Once::new();


/// Returns `true` if the given address is the exception handler in the current `IDT`
/// for any exception in which the CPU pushes an error code onto the stack.
/// 
/// On x86, only these exceptions cause the CPU to push error codes: 8, 10, 11, 12, 13, 14, 17, 29, 30.
/// 
/// Obtains a lock on the global `IDT` instance.
pub fn is_exception_handler_with_error_code(address: u64) -> bool {
    let idt = IDT.lock();
    let address = x86_64::VirtAddr::new_truncate(address);
    
    // These are sorted from most to least likely, in order to short-circuit sooner.
    idt.page_fault.handler_addr() == address 
        || idt.double_fault.handler_addr() == address
        || idt.general_protection_fault.handler_addr() == address
        || idt.invalid_tss.handler_addr() == address
        || idt.segment_not_present.handler_addr() == address
        || idt.stack_segment_fault.handler_addr() == address
        || idt.alignment_check.handler_addr() == address
        || idt.security_exception.handler_addr() == address
        || idt.vmm_communication_exception.handler_addr() == address
}


/// Initializes the interrupt subsystem and sets up an initial Interrupt Descriptor Table (IDT).
///
/// The new IDT will be initialized with the same contents as the early IDT 
/// created in [`exceptions_early::init()`].
/// Any other interrupt handler entries that are missing (not yet initialized) will be filled with
/// a default placeholder handler, which is useful to catch interrupts that need to be implemented.
///
/// # Arguments: 
/// * `double_fault_stack_top_unusable`: the address of the top of a newly allocated stack,
///    to be used as the double fault exception handler stack.
/// * `privilege_stack_top_unusable`: the address of the top of a newly allocated stack,
///    to be used as the privilege stack (Ring 3 -> Ring 0 stack).
pub fn init(
    double_fault_stack_top_unusable: VirtualAddress,
    privilege_stack_top_unusable: VirtualAddress
) -> Result<&'static LockedIdt, &'static str> {
    let bsp_id = apic::get_bsp_id().ok_or("couldn't get BSP's id")?;
    info!("Setting up TSS & GDT for BSP (id {})", bsp_id);
    gdt::create_and_load_tss_gdt(bsp_id, double_fault_stack_top_unusable, privilege_stack_top_unusable);

    // Before loading this new IDT, we must copy over all exception handlers from the early IDT.
    // However, we can't just clone `EARLY_IDT` into `IDT`, because we must 
    // preserve any handlers that were already registered to this `IDT` during early boot and device init.
    {
        let mut new_idt = IDT.lock();
        let early_idt = exceptions_early::EARLY_IDT.lock();

        new_idt.divide_error                = early_idt.divide_error;
        new_idt.debug                       = early_idt.debug;
        new_idt.non_maskable_interrupt      = early_idt.non_maskable_interrupt;
        new_idt.breakpoint                  = early_idt.breakpoint;
        new_idt.overflow                    = early_idt.overflow;
        new_idt.bound_range_exceeded        = early_idt.bound_range_exceeded;
        new_idt.invalid_opcode              = early_idt.invalid_opcode;
        new_idt.device_not_available        = early_idt.device_not_available;
        // double fault handler is dealt with below.
        new_idt.invalid_tss                 = early_idt.invalid_tss;
        new_idt.segment_not_present         = early_idt.segment_not_present;
        new_idt.stack_segment_fault         = early_idt.stack_segment_fault;
        new_idt.general_protection_fault    = early_idt.general_protection_fault;
        new_idt.page_fault                  = early_idt.page_fault;
        new_idt.x87_floating_point          = early_idt.x87_floating_point;
        new_idt.alignment_check             = early_idt.alignment_check;
        new_idt.machine_check               = early_idt.machine_check;
        new_idt.simd_floating_point         = early_idt.simd_floating_point;
        new_idt.virtualization              = early_idt.virtualization;
        new_idt.vmm_communication_exception = early_idt.vmm_communication_exception;
        new_idt.security_exception          = early_idt.security_exception;

        // The only special case is the double fault handler, 
        // as it needs to use the newly-provided double fault stack.
        let double_fault_options = new_idt.double_fault.set_handler_fn(exceptions_early::double_fault_handler);
        unsafe { 
            double_fault_options.set_stack_index(tss::DOUBLE_FAULT_IST_INDEX as u16);
        }

        // Fill only *missing* IDT entries with a default unimplemented interrupt handler.
        for (_idx, new_entry) in new_idt.slice_mut(32..=255).iter_mut().enumerate() {
            if new_entry.handler_addr().as_u64() != 0 {
                debug!("Preserved early registered interrupt handler for IRQ {:#X} at address {:#X}", 
                    _idx + IRQ_BASE_OFFSET as usize, new_entry.handler_addr(),
                );
            } else {
                new_entry.set_handler_fn(unimplemented_interrupt_handler);
            }
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
pub fn init_ap(
    apic_id: u8, 
    double_fault_stack_top_unusable: VirtualAddress, 
    privilege_stack_top_unusable: VirtualAddress,
) -> Result<&'static LockedIdt, &'static str> {
    info!("Setting up TSS & GDT for AP {}", apic_id);
    gdt::create_and_load_tss_gdt(apic_id, double_fault_stack_top_unusable, privilege_stack_top_unusable);

    // We've already created the IDT initially (currently all APs share the BSP's IDT),
    // so we only need to re-load it here for each AP.
    IDT.load();
    info!("loaded IDT for AP {}.", apic_id);
    Ok(&IDT)
}


/// Establishes the default interrupt handlers that are statically known.
fn set_handlers(idt: &mut InterruptDescriptorTable) {
    idt[0x20].set_handler_fn(pit_timer_handler);
    idt[0x21].set_handler_fn(ps2_keyboard_handler);
    idt[0x22].set_handler_fn(lapic_timer_handler);
    idt[0x27].set_handler_fn(pic_spurious_interrupt_handler); 

    // idt[0x28].set_handler_fn(rtc_handler);
    idt[0x2C].set_handler_fn(ps2_mouse_handler);
    idt[0x2E].set_handler_fn(primary_ata_handler);
    idt[0x2F].set_handler_fn(secondary_ata_handler);

    idt[apic::APIC_SPURIOUS_INTERRUPT_VECTOR as usize].set_handler_fn(apic_spurious_interrupt_handler); 
    idt[tlb_shootdown::TLB_SHOOTDOWN_IPI_IRQ as usize].set_handler_fn(ipi_handler);
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
/// * `interrupt_num`: the interrupt (IRQ vector) that is being requested.
/// * `func`: the handler to be registered, which will be invoked when the interrupt occurs.
/// 
/// # Return
/// * `Ok(())` if successfully registered, or
/// * `Err(existing_handler_address)` if the given `interrupt_num` was already in use.
pub fn register_interrupt(interrupt_num: u8, func: HandlerFunc) -> Result<(), u64> {
    let mut idt = IDT.lock();

    // If the existing handler stored in the IDT either missing (has an address of `0`)
    // or is the default handler, that signifies the interrupt number is available.
    let idt_entry = &mut idt[interrupt_num as usize];
    let existing_handler_addr = idt_entry.handler_addr().as_u64();
    if existing_handler_addr == 0 || existing_handler_addr == unimplemented_interrupt_handler as u64 {
        idt_entry.set_handler_fn(func);
        Ok(())
    } else {
        trace!("register_interrupt: the requested interrupt IRQ {} was already in use", interrupt_num);
        Err(existing_handler_addr)
    }
} 

/// Returns an interrupt number assigned by the OS and sets its handler function. 
/// The function fails if there is no unused interrupt number.
/// 
/// # Arguments
/// * `func` - the handler for the assigned interrupt number
pub fn register_msi_interrupt(func: HandlerFunc) -> Result<u8, &'static str> {
    let mut idt = IDT.lock();

    // try to find an unused interrupt number in the IDT
    let interrupt_num = idt.slice(32..=255)
        .iter()
        .rposition(|&entry| entry.handler_addr().as_u64() == unimplemented_interrupt_handler as u64)
        .map(|entry| entry + 32)
        .ok_or("register_msi_interrupt: no available interrupt handlers (BUG: IDT is full?)")?;

    idt[interrupt_num].set_handler_fn(func);
    
    Ok(interrupt_num as u8)
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
    if idt[interrupt_num as usize].handler_addr().as_u64() == func as u64 {
        idt[interrupt_num as usize].set_handler_fn(unimplemented_interrupt_handler);
        Ok(())
    }
    else {
        error!("deregister_interrupt: Cannot free interrupt due to incorrect handler function");
        Err("deregister_interrupt: Cannot free interrupt due to incorrect handler function")
    }
}

/// Send an end of interrupt signal, notifying the interrupt chip that
/// the given interrupt request `irq` has been serviced. 
/// 
/// This function supports all types of interrupt chips -- APIC, x2apic, PIC --
/// and will perform the correct EOI operation based on which chip is currently active.
///
/// The `irq` argument is only used if the `PIC` chip is active,
/// but it doesn't hurt to always provide it.
pub fn eoi(irq: Option<u8>) {
    match INTERRUPT_CHIP.load() {
        InterruptChip::APIC | InterruptChip::X2APIC => {
            if let Some(my_apic) = apic::get_my_apic() {
                my_apic.write().eoi();
            } else {
                error!("BUG: couldn't get my LocalApic instance to send EOI!");
            }
        }
        InterruptChip::PIC => {
            if let Some(_pic) = PIC.get() {
                if let Some(irq) = irq {
                    _pic.notify_end_of_interrupt(irq);
                } else {
                    error!("BUG: missing required IRQ argument for PIC EOI!");
                }   
            } else {
                error!("BUG: couldn't get PIC instance to send EOI!");
            }  
        }
    }
}


/// 0x20
extern "x86-interrupt" fn pit_timer_handler(_stack_frame: InterruptStackFrame) {
    pit_clock::handle_timer_interrupt();

	eoi(Some(IRQ_BASE_OFFSET + 0x0));
}


// see this: https://forum.osdev.org/viewtopic.php?f=1&t=32655
static EXTENDED_SCANCODE: AtomicBool = AtomicBool::new(false);

/// 0x21
extern "x86-interrupt" fn ps2_keyboard_handler(_stack_frame: InterruptStackFrame) {

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
    
    eoi(Some(IRQ_BASE_OFFSET + 0x1));
}

/// 0x2C
extern "x86-interrupt" fn ps2_mouse_handler(_stack_frame: InterruptStackFrame) {

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

    eoi(Some(IRQ_BASE_OFFSET + 0xc));
}

pub static APIC_TIMER_TICKS: AtomicUsize = AtomicUsize::new(0);
/// 0x22
extern "x86-interrupt" fn lapic_timer_handler(_stack_frame: InterruptStackFrame) {
    let _ticks = APIC_TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    // info!(" ({}) APIC TIMER HANDLER! TICKS = {}", apic::get_my_apic_id(), _ticks);

    // Callback to the sleep API to unblock tasks whose waiting time is over
    // and alert to update the number of ticks elapsed
    sleep::increment_tick_count();
    sleep::unblock_sleeping_tasks();
    
    // we must acknowledge the interrupt first before handling it because we switch tasks here, which doesn't return
    eoi(None); // None, because 0x22 IRQ cannot possibly be a PIC interrupt
    
    scheduler::schedule();
}

extern "x86-interrupt" fn apic_spurious_interrupt_handler(_stack_frame: InterruptStackFrame) {
    warn!("APIC SPURIOUS INTERRUPT HANDLER!");

    eoi(None);
}

extern "x86-interrupt" fn unimplemented_interrupt_handler(_stack_frame: InterruptStackFrame) {
    println_raw!("\nUnimplemented interrupt handler: {:#?}", _stack_frame);
	match apic::INTERRUPT_CHIP.load() {
        apic::InterruptChip::PIC => {
            let irq_regs = PIC.get().map(|pic| pic.read_isr_irr());  
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

    // TODO: use const generics here to know which IRQ to send an EOI for (only needed for PIC).
    eoi(None); 
}



/// The Spurious interrupt handler for the PIC. 
/// This has given us a lot of problems on bochs emulator and on some real hardware, but not on QEMU.
/// Spurious interrupts occur a lot when using PIC on real hardware, but only occurs once when using apic/x2apic. 
/// See here for more: https://mailman.linuxchix.org/pipermail/techtalk/2002-August/012697.html.
/// We handle it according to this advice: https://wiki.osdev.org/8259_PIC#Spurious_IRQs
extern "x86-interrupt" fn pic_spurious_interrupt_handler(_stack_frame: InterruptStackFrame ) {
    if let Some(pic) = PIC.get() {
        let irq_regs = pic.read_isr_irr();
        // check if this was a real IRQ7 (parallel port) (bit 7 will be set)
        // (pretty sure this will never happen)
        // if it was a real IRQ7, we do need to ack it by sending an EOI
        if irq_regs.master_isr & 0x80 == 0x80 {
            println_raw!("\nGot real IRQ7, not spurious! (Unexpected behavior)");
            error!("Got real IRQ7, not spurious! (Unexpected behavior)");
            eoi(Some(IRQ_BASE_OFFSET + 0x7));
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
// extern "x86-interrupt" fn rtc_handler(_stack_frame: InterruptStackFrame ) {
//     // because we use the RTC interrupt handler for task switching,
//     // we must ack the interrupt and send EOI before calling the handler, 
//     // because the handler will not return.
//     rtc::rtc_ack_irq();
//     eoi(Some(IRQ_BASE_OFFSET + 0x8));
    
//     rtc::handle_rtc_interrupt();
// }


/// 0x2E
extern "x86-interrupt" fn primary_ata_handler(_stack_frame: InterruptStackFrame ) {
    info!("Primary ATA Interrupt (0x2E)");

    eoi(Some(IRQ_BASE_OFFSET + 0xE));
}


/// 0x2F
extern "x86-interrupt" fn secondary_ata_handler(_stack_frame: InterruptStackFrame ) {
    info!("Secondary ATA Interrupt (0x2F)");
    
    eoi(Some(IRQ_BASE_OFFSET + 0xF));
}


extern "x86-interrupt" fn ipi_handler(_stack_frame: InterruptStackFrame) {
    eoi(None);
}
