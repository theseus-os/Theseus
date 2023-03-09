//! Exception handlers that are task-aware, and will kill a task on an exception.

// TODO: Add direct explanation to why this empty loop is necessary and criteria for replacing it with something else
#![allow(clippy::empty_loop)]
#![no_std]
#![feature(abi_x86_interrupt)]

use log::{warn, debug, trace};
use memory::{VirtualAddress, Page};
use signal_handler::{Signal, SignalContext, ErrorCode};
use x86_64::{
    registers::control::Cr2,
    structures::idt::{
        InterruptStackFrame,
        PageFaultErrorCode
    },
};
use locked_idt::LockedIdt;
use fault_log::log_exception;


/// Initialize the given `idt` with fully-featured exception handlers.
/// 
/// This only sets the exception `Entry`s in the `IDT`, i.e.,
/// entries from `0` to `31` (inclusive).
/// Entries from `32` to `255` (inclusive) are not modified, 
/// as those are for custom OS-specfici interrupt handlers.
pub fn init(idt_ref: &'static LockedIdt) {
    { 
        let mut idt = idt_ref.lock(); // withholds interrupts

        // SET UP FIXED EXCEPTION HANDLERS
        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.debug.set_handler_fn(debug_handler);
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.overflow.set_handler_fn(overflow_handler);
        idt.bound_range_exceeded.set_handler_fn(bound_range_exceeded_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.device_not_available.set_handler_fn(device_not_available_handler);
        let options = idt.double_fault.set_handler_fn(double_fault_handler);
        unsafe { 
            options.set_stack_index(tss::DOUBLE_FAULT_IST_INDEX as u16);
        }
        // reserved: 0x09 coprocessor segment overrun exception
        idt.invalid_tss.set_handler_fn(invalid_tss_handler);
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        // reserved: 0x0F
        idt.x87_floating_point.set_handler_fn(x87_floating_point_handler);
        idt.alignment_check.set_handler_fn(alignment_check_handler);
        idt.machine_check.set_handler_fn(machine_check_handler);
        idt.simd_floating_point.set_handler_fn(simd_floating_point_handler);
        idt.virtualization.set_handler_fn(virtualization_handler);
        // reserved: 0x15 - 0x1C
        idt.vmm_communication_exception.set_handler_fn(vmm_communication_exception_handler);
        idt.security_exception.set_handler_fn(security_exception_handler);
        // reserved: 0x1F
    }

    idt_ref.load();
}


/// calls print!() and then print_raw!()
macro_rules! println_both {
    ($fmt:expr) => {
        vga_buffer::print_raw!(concat!($fmt, "\n"));
        app_io::print!(concat!($fmt, "\n"));
    };
    ($fmt:expr, $($arg:tt)*) => {
        vga_buffer::print_raw!(concat!($fmt, "\n"), $($arg)*);
        app_io::print!(concat!($fmt, "\n"), $($arg)*);
    };
}


/// Kills the current task (the one that caused an exception) by unwinding it.
/// 
/// # Important Note
/// Currently, unwinding a task after an exception does not fully work like it does for panicked tasks.
/// The problem is that unwinding cleanup routines (landing pads) are generated *only if* a panic can actually occur. 
/// Since machine exceptions can occur anywhere at any time (beneath the language level),
/// 
/// Currently, what will happen is that all stack frames will be unwound properly **except**
/// for the one during which the exception actually occurred; 
/// the "excepted"/interrupted frame may be cleaned up properly, but it is unlikely. 
/// 
/// However, stack traces / backtraces work, so we are correctly traversing call stacks with exception frames.
/// 
#[inline(never)]
fn kill_and_halt(
    exception_number: u8,
    stack_frame: &InterruptStackFrame,
    error_code: Option<ErrorCode>,
    print_stack_trace: bool
) {
    // First, log the exception that merits a kill operation.
    {
        let (err, addr) = match error_code {
            Some(ErrorCode::PageFaultError {accessed_address, pf_error}) => (Some(pf_error.bits()), Some(accessed_address)),
            Some(ErrorCode::Other(e)) => (Some(e), None),
            None => (None, None),
        };
        log_exception(exception_number, stack_frame.instruction_pointer.as_u64() as usize, err, addr);
    }


    #[cfg(all(unwind_exceptions, not(downtime_eval)))] {
        println_both!("Unwinding {:?} due to exception {}.", task::get_my_current_task(), exception_number);
    }
    #[cfg(not(unwind_exceptions))] {
        println_both!("Killing task without unwinding {:?} due to exception {}. (cfg `unwind_exceptions` is not set.)", task::get_my_current_task(), exception_number);
    }
    
    // Dump some info about the this loaded app crate
    // and test out using debug info for recovery
    if false {
        let curr_task = task::get_my_current_task().expect("kill_and_halt: no current task");
        let app_crate = curr_task.app_crate.as_ref().expect("kill_and_halt: no app_crate").clone_shallow();
        let debug_symbols_file = {
            let krate = app_crate.lock_as_ref();
            trace!("============== Crate {} =================", krate.crate_name);
            for s in krate.sections.values() {
                trace!("   {:?}", s);
            }
            krate.debug_symbols_file.clone()
        };

        if false {
            let mut debug = debug_info::DebugSymbols::Unloaded(debug_symbols_file);
            let debug_sections = debug.load(&app_crate, curr_task.get_namespace()).unwrap();
            let instr_ptr = stack_frame.instruction_pointer.as_u64() as usize - 1; // points to the next instruction (at least for a page fault)

            let res = debug_sections.find_subprogram_containing(VirtualAddress::new_canonical(instr_ptr));
            debug!("Result of find_subprogram_containing: {:?}", res);
        }
    }

    // print a stack trace
    #[cfg(not(downtime_eval))] {
        if print_stack_trace {
            println_both!("------------------ Stack Trace (DWARF) ---------------------------");
            let stack_trace_result = stack_trace::stack_trace(
                &mut |stack_frame, stack_frame_iter| {
                    let symbol_offset = stack_frame_iter.namespace().get_section_containing_address(
                        VirtualAddress::new_canonical(stack_frame.call_site_address() as usize),
                        false
                    ).map(|(sec, offset)| (sec.name.clone(), offset));
                    if let Some((symbol_name, offset)) = symbol_offset {
                        println_both!("  {:>#018X} in {} + {:#X}", stack_frame.call_site_address(), symbol_name, offset);
                    } else {
                        println_both!("  {:>#018X} in ??", stack_frame.call_site_address());
                    }
                    true
                },
                None,
            );
            match stack_trace_result {
                Ok(()) => { println_both!("  Beginning of stack"); }
                Err(e) => { println_both!("  {}", e); }
            }
            println_both!("---------------------- End of Stack Trace ------------------------");
        }
    }

    let cause = task::KillReason::Exception(exception_number);

    // Call this task's kill handler, if it has one.
    if let Some(ref kh_func) = task::take_kill_handler() {
        #[cfg(not(downtime_eval))]
        debug!("Found kill handler callback to invoke in Task {:?}", task::get_my_current_task());
        kh_func(&cause);
    } else {
        #[cfg(not(downtime_eval))]
        debug!("No kill handler callback in Task {:?}", task::get_my_current_task());
    }

    // Invoke the proper signal handler registered for this task, if one exists.
    if let Some(signal) = exception_to_signal(exception_number) {
        if let Some(handler) = signal_handler::take_signal_handler(signal) {
            warn!("Invoking signal handler for {:?}", signal);
            let signal_context = SignalContext {
                instruction_pointer: VirtualAddress::new_canonical(stack_frame.instruction_pointer.as_u64() as usize),
                stack_pointer: VirtualAddress::new_canonical(stack_frame.stack_pointer.as_u64() as usize),
                signal,
                error_code,
            };
            if handler(&signal_context).is_ok() {
                warn!("Signal handler for {:?} returned Ok. Returning from exception handler is disabled and untested.", signal);
                // TODO: test and enable this return;
            }
        }
    }

    // Unwind the current task that failed due to the given exception.
    // This doesn't always work perfectly, so it's disabled by default for now.
    #[cfg(unwind_exceptions)] {
        // skip 2 frames: `start_unwinding` and `kill_and_halt`
        match unwind::start_unwinding(cause, 2) {
            Ok(_) => {
                println_both!("BUG: when handling exception {}, start_unwinding() returned an Ok() value, \
                    which is unexpected because it means no unwinding actually occurred. Task: {:?}.", 
                    exception_number,
                    task::get_my_current_task()
                );
            }
            Err(e) => {
                println_both!("Task {:?} was unable to start unwinding procedure after exception {}, error: {}.",
                    task::get_my_current_task(), exception_number, e
                );
            }
        }
    }
    #[cfg(not(unwind_exceptions))] {
        let res = task::with_current_task(|t| {
            let kill_result = t.kill(cause);
            match kill_result {
                Ok(()) => { println_both!("Task {:?} killed itself successfully", t); }
                Err(e) => { println_both!("Task {:?} was unable to kill itself. Error: {:?}", t, e); }
            }
            kill_result
        });
        if res.is_err() {
            println_both!("BUG: kill_and_halt(): Couldn't get current task in order to kill it.");
        }
    }

    // If we failed to handle the exception and unwind the task, there's not really much we can do about it,
    // other than just let the thread spin endlessly (which doesn't hurt correctness but is inefficient). 
    // But in general, this task should have already been marked as killed and thus no longer schedulable,
    // so it should not reach this point. 
    // Only exceptions during the early OS initialization process will get here, meaning that the OS will basically stop.
    loop { }
}


/// Checks whether the given `vaddr` falls within a stack guard page, indicating stack overflow. 
fn is_stack_overflow(vaddr: VirtualAddress) -> bool {
    let page = Page::containing_address(vaddr);
    task::with_current_task(|t|
        t.with_kstack(|kstack| kstack.guard_page().contains(&page))
    ).unwrap_or(false)
}

/// Converts the given `exception_number` into a [`Signal`] category, if relevant.
fn exception_to_signal(exception_number: u8) -> Option<Signal> {
    match exception_number {
        0x00 | 0x04 | 0x10 | 0x13         => Some(Signal::ArithmeticError),
        0x05 | 0x0E | 0x0C                => Some(Signal::InvalidAddress),
        0x06 | 0x07 | 0x08 | 0x0A | 0x0D  => Some(Signal::IllegalInstruction),
        0x0B | 0x11                       => Some(Signal::BusError),
        _                                 => None,
    }
}


/// exception 0x00
extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    println_both!("\nEXCEPTION: DIVIDE ERROR\n{:#X?}\n", stack_frame);
    kill_and_halt(0x0, &stack_frame, None, true)
}

/// exception 0x01
extern "x86-interrupt" fn debug_handler(stack_frame: InterruptStackFrame) {
    println_both!("\nEXCEPTION: DEBUG EXCEPTION\n{:#X?}", stack_frame);
    // don't halt here, this isn't a fatal/permanent failure, just a brief pause.
}

/// exception 0x02, also used for TLB Shootdown IPIs and sampling interrupts.
///
/// # Important Note
/// Acquiring ANY locks in this function, even irq-safe ones, could cause a deadlock
/// because this interrupt takes priority over everything else and can interrupt
/// another regular interrupt. 
/// This includes printing to the log (e.g., `debug!()`) or the screen.
extern "x86-interrupt" fn nmi_handler(stack_frame: InterruptStackFrame) {
    let mut expected_nmi = false;

    // currently we're using NMIs to send TLB shootdown IPIs
    {
        let pages_to_invalidate = tlb_shootdown::TLB_SHOOTDOWN_IPI_PAGES.read().clone();
        if let Some(pages) = pages_to_invalidate {
            // trace!("nmi_handler (AP {})", cpu::current_cpu());
            tlb_shootdown::handle_tlb_shootdown_ipi(pages);
            expected_nmi = true;
        }
    }

    // Performance monitoring hardware uses NMIs to trigger a sampling interrupt.
    match pmu_x86::handle_sample(&stack_frame) {
        // A PMU sample did occur and was properly handled, so this NMI was expected. 
        Ok(true) => expected_nmi = true,
        // No PMU sample occurred, so this NMI was unexpected.
        Ok(false) => { }
        // A PMU sample did occur but wasn't properly handled, so this NMI was expected. 
        Err(_e) => {
            println_both!("nmi_handler: pmu_x86 failed to record sample: {:?}", _e);
            expected_nmi = true;
        }
    }

    if expected_nmi {
        return;
    }

    println_both!("\nEXCEPTION: NON-MASKABLE INTERRUPT at {:#X}\n{:#X?}\n",
        stack_frame.instruction_pointer,
        stack_frame,
    );

    log_exception(0x2, stack_frame.instruction_pointer.as_u64() as usize, None, None);
    kill_and_halt(0x2, &stack_frame, None, true)
}


/// exception 0x03
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println_both!("\nEXCEPTION: BREAKPOINT\n{:#X?}", stack_frame);
    // don't halt here, this isn't a fatal/permanent failure, just a brief pause.
}

/// exception 0x04
extern "x86-interrupt" fn overflow_handler(stack_frame: InterruptStackFrame) {
    println_both!("\nEXCEPTION: OVERFLOW\n{:#X?}", stack_frame);
    kill_and_halt(0x4, &stack_frame, None, true)
}

// exception 0x05
extern "x86-interrupt" fn bound_range_exceeded_handler(stack_frame: InterruptStackFrame) {
    println_both!("\nEXCEPTION: BOUND RANGE EXCEEDED\n{:#X?}", stack_frame);
    kill_and_halt(0x5, &stack_frame, None, true)
}

/// exception 0x06
extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    println_both!("\nEXCEPTION: INVALID OPCODE\n{:#X?}", stack_frame);
    kill_and_halt(0x6, &stack_frame, None, true)
}

/// exception 0x07
///
/// For more information about "spurious interrupts", 
/// see [here](http://wiki.osdev.org/I_Cant_Get_Interrupts_Working#I_keep_getting_an_IRQ7_for_no_apparent_reason).
extern "x86-interrupt" fn device_not_available_handler(stack_frame: InterruptStackFrame) {
    println_both!("\nEXCEPTION: DEVICE NOT AVAILABLE\n{:#X?}", stack_frame);
    kill_and_halt(0x7, &stack_frame, None, true)
}

/// exception 0x08
extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) -> ! {
    let accessed_vaddr = Cr2::read_raw();
    println_both!("\nEXCEPTION: DOUBLE FAULT\n{:#X?}\nTried to access {:#X}
        Note: double faults in Theseus are typically caused by stack overflow, is the stack large enough?",
        stack_frame, accessed_vaddr,
    );
    if is_stack_overflow(VirtualAddress::new_canonical(accessed_vaddr as usize)) {
        println_both!("--> This double fault was definitely caused by stack overflow, tried to access {:#X}.\n", accessed_vaddr);
    }
    
    kill_and_halt(0x8, &stack_frame, Some(error_code.into()), false);
    loop {}
}

/// exception 0x0A
extern "x86-interrupt" fn invalid_tss_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: INVALID TSS\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    kill_and_halt(0xA, &stack_frame, Some(error_code.into()), true)
}

/// exception 0x0B
extern "x86-interrupt" fn segment_not_present_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: SEGMENT NOT PRESENT\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    kill_and_halt(0xB, &stack_frame, Some(error_code.into()), true)
}

/// exception 0x0C
extern "x86-interrupt" fn stack_segment_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: STACK SEGMENT FAULT\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    kill_and_halt(0xC, &stack_frame, Some(error_code.into()), true)
}

/// exception 0x0D
extern "x86-interrupt" fn general_protection_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: GENERAL PROTECTION FAULT\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    kill_and_halt(0xD, &stack_frame, Some(error_code.into()), true)
}

/// exception 0x0E
extern "x86-interrupt" fn page_fault_handler(stack_frame: InterruptStackFrame, error_code: PageFaultErrorCode) {
    let accessed_vaddr = Cr2::read_raw() as usize;

    #[cfg(not(downtime_eval))] {
        println_both!("\nEXCEPTION: PAGE FAULT while accessing {:#x}\n\
            error code: {:?}\n{:#X?}",
            accessed_vaddr,
            error_code,
            stack_frame
        );
        if is_stack_overflow(VirtualAddress::new_canonical(accessed_vaddr)) {
            println_both!("--> Page fault was caused by stack overflow, tried to access {:#X}\n.", accessed_vaddr);
        }
    }
    
    kill_and_halt(0xE, &stack_frame, Some(ErrorCode::PageFaultError { accessed_address: accessed_vaddr, pf_error: error_code }), true)
}


/// exception 0x10
extern "x86-interrupt" fn x87_floating_point_handler(stack_frame: InterruptStackFrame) {
    println_both!("\nEXCEPTION: x87 FLOATING POINT\n{:#X?}", stack_frame);
    kill_and_halt(0x10, &stack_frame, None, true)
}

/// exception 0x11
extern "x86-interrupt" fn alignment_check_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: ALIGNMENT CHECK\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    kill_and_halt(0x11, &stack_frame, Some(error_code.into()), true)
}

/// exception 0x12
extern "x86-interrupt" fn machine_check_handler(stack_frame: InterruptStackFrame) -> ! {
    println_both!("\nEXCEPTION: MACHINE CHECK\n{:#X?}", stack_frame);
    kill_and_halt(0x12, &stack_frame, None, true);
    loop {}
}

/// exception 0x13
extern "x86-interrupt" fn simd_floating_point_handler(stack_frame: InterruptStackFrame) {
    println_both!("\nEXCEPTION: SIMD FLOATING POINT\n{:#X?}", stack_frame);
    kill_and_halt(0x13, &stack_frame, None, true)
}

/// exception 0x14
extern "x86-interrupt" fn virtualization_handler(stack_frame: InterruptStackFrame) {
    println_both!("\nEXCEPTION: VIRTUALIZATION\n{:#X?}", stack_frame);
    kill_and_halt(0x14, &stack_frame, None, true)
}

/// exception 0x1D
extern "x86-interrupt" fn vmm_communication_exception_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: VMM COMMUNICATION EXCEPTION\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    kill_and_halt(0x1D, &stack_frame, Some(error_code.into()),true)
}

/// exception 0x1E
extern "x86-interrupt" fn security_exception_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: SECURITY EXCEPTION\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    kill_and_halt(0x1E, &stack_frame, Some(error_code.into()), true)
}
