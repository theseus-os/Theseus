pub use pic::IRQ_BASE_OFFSET;

// use rtc;
use apic::{INTERRUPT_CHIP, InterruptChip};
use cpu::CpuId;
use locked_idt::LockedIdt;
use log::{error, warn, info, debug};
use memory::VirtualAddress;
use spin::Once;
use early_printer::println;
pub use x86_64::structures::idt::{InterruptStackFrame, HandlerFunc};

/// The IRQ number reserved for CPU-local timer interrupts,
/// which Theseus currently uses for preemptive task switching.
pub const CPU_LOCAL_TIMER_IRQ: u8 = apic::LOCAL_APIC_LVT_IRQ;

/// The single system-wide Interrupt Descriptor Table (IDT).
///
/// Note: this could be per-core instead of system-wide, if needed.
pub static IDT: LockedIdt = LockedIdt::new();

/// The single system-wide Programmable Interrupt Controller (PIC) chip.
static PIC: Once<pic::ChainedPics> = Once::new();

/// The list of IRQs reserved for Theseus-specific usage that cannot be
/// used for general device interrupt handlers.
/// These cannot be removed in [`deregister_interrupt()`].
static RESERVED_IRQ_LIST: [u8; 3] = [
    pic::PIC_SPURIOUS_INTERRUPT_IRQ,
    CPU_LOCAL_TIMER_IRQ,
    apic::APIC_SPURIOUS_INTERRUPT_IRQ,
];


#[macro_export]
macro_rules! interrupt_handler {
    ($name:ident, $x86_64_interrupt_number:expr, $stack_frame:ident, $code:block) => {
        extern "x86-interrupt" fn $name(sf: $crate::InterruptStackFrame) {
            let $stack_frame = &sf;
            if let $crate::EoiBehavior::HandlerDidNotSendEoi = $code {
                $crate::eoi($x86_64_interrupt_number);
            }
        }
    }
}


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
    let bsp_id = cpu::bootstrap_cpu().ok_or("couldn't get BSP's id")?;
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
        // This crate has a fixed dependency on the `pic` and `apic` crates,
        // because they are required to implement certain functions, e.g., `eoi()`.
        // Thus, we statically reserve some non-registerable IDT entries that should only be used
        // by the PIC & APIC instead of dynamically registering them elsewhere.
        new_idt[pic::PIC_SPURIOUS_INTERRUPT_IRQ as usize]
            .set_handler_fn(pic_spurious_interrupt_handler);
        new_idt[apic::APIC_SPURIOUS_INTERRUPT_IRQ as usize]
            .set_handler_fn(apic_spurious_interrupt_handler);
    }

    // try to load our new IDT    
    info!("trying to load IDT for BSP...");
    IDT.load();
    info!("loaded IDT for BSP.");

    // Use the APIC instead of the old PIC
    disable_pic();

    Ok(&IDT)
}


/// Similar to `init()`, but for APs to call after the BSP has already invoked `init()`.
pub fn init_ap(
    cpu_id: CpuId, 
    double_fault_stack_top_unusable: VirtualAddress, 
    privilege_stack_top_unusable: VirtualAddress,
) -> Result<&'static LockedIdt, &'static str> {
    info!("Setting up TSS & GDT for CPU {}", cpu_id);
    gdt::create_and_load_tss_gdt(cpu_id, double_fault_stack_top_unusable, privilege_stack_top_unusable);

    // We've already created the IDT initially (currently all CPUs share the initial IDT),
    // so we only need to re-load it here for each AP (each secondary CPU).
    IDT.load();
    info!("loaded IDT for CPU {}.", cpu_id);
    Ok(&IDT)
}

/// Disables the PIC by masking all of its interrupts, indicating this system uses an APIC.
fn disable_pic() {
    PIC.call_once(|| pic::ChainedPics::init(0xFF, 0xFF)); // disable all PIC IRQs
}

/// Enable the PIC by enabling all of its interrupts.
/// This indicates the system does not have an APIC or that we don't wish to use it.
/// 
/// Note: currently we assume all systems have an APIC, so this is not used.
///       If we ever did re-enable it, we would also need to set up PIT/RTC timer interrupts
///       for preemptive task switching instead of the APIC LVT timer.
fn _enable_pic() {
    let master_pic_mask: u8 = 0x0; // allow every interrupt
    let slave_pic_mask: u8 = 0b0000_1000; // everything is allowed except 0x2B 
    PIC.call_once(|| pic::ChainedPics::init(master_pic_mask, slave_pic_mask));

    // pit_clock::init(CONFIG_PIT_FREQUENCY_HZ);
    // let rtc_handler = rtc::init(CONFIG_RTC_FREQUENCY_HZ, rtc_interrupt_func);
    // IDT.lock()[0x28].set_handler_fn(rtc_handler.unwrap());
}

/// Registers an interrupt handler at the given IRQ interrupt number.
///
/// The function fails if the interrupt number is reserved or is already in use.
///
/// # Arguments 
/// * `interrupt_num`: the interrupt (IRQ vector) that is being requested.
/// * `func`: the handler to be registered, which will be invoked when the interrupt occurs.
///
/// # Return
/// * `Ok(())` if successfully registered, or
/// * `Err(existing_handler_address)` if the given `interrupt_num` was already in use.
pub fn register_interrupt(interrupt_num: u8, func: HandlerFunc) -> Result<(), usize> {
    let mut idt = IDT.lock();

    // If the existing handler stored in the IDT is either missing (has an address of `0`)
    // or is the default handler, that signifies the interrupt number is available.
    let idt_entry = &mut idt[interrupt_num as usize];
    let existing_handler_addr = idt_entry.handler_addr().as_u64() as usize;
    if existing_handler_addr == 0 || existing_handler_addr == unimplemented_interrupt_handler as usize {
        idt_entry.set_handler_fn(func);
        Ok(())
    } else {
        error!("register_interrupt: the requested interrupt IRQ {} was already in use", interrupt_num);
        Err(existing_handler_addr)
    }
} 

/// Allocates and returns an unused interrupt number and sets its handler function.
///
/// Returns an error if there are no unused interrupt number, which is highly unlikely.
///
/// # Arguments
/// * `func`: the handler for the assigned interrupt number.
pub fn register_msi_interrupt(func: HandlerFunc) -> Result<u8, &'static str> {
    let mut idt = IDT.lock();

    // try to find an unused interrupt number in the IDT
    let interrupt_num = idt.slice(32..=255)
        .iter()
        .rposition(|&entry| entry.handler_addr().as_u64() as usize == unimplemented_interrupt_handler as usize)
        .map(|entry| entry + 32)
        .ok_or("register_msi_interrupt: no available interrupt handlers (BUG: IDT is full?)")?;

    idt[interrupt_num].set_handler_fn(func);
    
    Ok(interrupt_num as u8)
} 

/// Deregisters an interrupt handler, making it available to the rest of the system again.
///
/// As a sanity/safety check, the caller must provide the `interrupt_handler`
/// that is currently registered for the given IRQ `interrupt_num`.
/// This function returns an error if the currently-registered handler does not match 'func'.
///
/// # Arguments
/// * `interrupt_num`: the IRQ that needs to be deregistered
/// * `func`: the handler that should currently be stored for 'interrupt_num'
pub fn deregister_interrupt(interrupt_num: u8, func: HandlerFunc) -> Result<(), &'static str> {
    let mut idt = IDT.lock();

    if RESERVED_IRQ_LIST.contains(&interrupt_num) {
        error!("deregister_interrupt: Cannot free reserved interrupt number, IRQ {}", interrupt_num);
        return Err("deregister_interrupt: Cannot free reserved interrupt IRQ number");
    }

    // check if the handler stored is the same as the one provided
    // this is to make sure no other application can deregister your interrupt
    if idt[interrupt_num as usize].handler_addr().as_u64() as usize == func as usize {
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


extern "x86-interrupt" fn apic_spurious_interrupt_handler(_stack_frame: InterruptStackFrame) {
    warn!("APIC SPURIOUS INTERRUPT HANDLER!");

    eoi(None);
}

extern "x86-interrupt" fn unimplemented_interrupt_handler(_stack_frame: InterruptStackFrame) {
    println!("\nUnimplemented interrupt handler: {:#?}", _stack_frame);
	match apic::INTERRUPT_CHIP.load() {
        apic::InterruptChip::PIC => {
            let irq_regs = PIC.get().map(|pic| pic.read_isr_irr());  
            println!("PIC IRQ Registers: {:?}", irq_regs);
        }
        apic::InterruptChip::APIC | apic::InterruptChip::X2APIC => {
            if let Some(lapic_ref) = apic::get_my_apic() {
                let lapic = lapic_ref.read();
                let isr = lapic.get_isr(); 
                let irr = lapic.get_irr();
                println!("APIC ISR: {:#x} {:#x} {:#x} {:#x}, {:#x} {:#x} {:#x} {:#x}\n \
                    IRR: {:#x} {:#x} {:#x} {:#x},{:#x} {:#x} {:#x} {:#x}", 
                    isr[0], isr[1], isr[2], isr[3], isr[4], isr[5], isr[6], isr[7],
                    irr[0], irr[1], irr[2], irr[3], irr[4], irr[5], irr[6], irr[7],
                );
            }
            else {
                println!("APIC ISR and IRR were unknown.");
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
            println!("\nGot real IRQ7, not spurious! (Unexpected behavior)");
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
