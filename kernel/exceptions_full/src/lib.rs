//! Exception handlers that are task-aware, and will kill a task on an exception.

#![no_std]
#![feature(abi_x86_interrupt)]

extern crate alloc;
extern crate x86_64;
extern crate task;
// extern crate apic;
extern crate tlb_shootdown;
extern crate pmu_x86;
#[macro_use] extern crate log;
#[macro_use] extern crate vga_buffer; // for println_raw!()
#[macro_use] extern crate print; // for regular println!()
extern crate unwind;
extern crate debug_info;
extern crate gimli;

extern crate memory;
extern crate stack_trace;
extern crate fault_log;
extern crate mod_mgmt;
extern crate apic;

use x86_64::structures::idt::{LockedIdt, ExceptionStackFrame, PageFaultErrorCode};
use x86_64::registers::msr::*;
use apic::get_my_apic_id;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use fault_log::{
    add_error_to_fault_log,
    add_error_simple,
};

use memory::VirtualAddress;

pub fn init(idt_ref: &'static LockedIdt) {
    { 
        let mut idt = idt_ref.lock(); // withholds interrupts

        // SET UP FIXED EXCEPTION HANDLERS
        idt.divide_by_zero.set_handler_fn(divide_by_zero_handler);
        idt.debug.set_handler_fn(debug_handler);
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.overflow.set_handler_fn(overflow_handler);
        idt.bound_range_exceeded.set_handler_fn(bound_range_exceeded_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.device_not_available.set_handler_fn(device_not_available_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        // reserved: 0x09 coprocessor segment overrun exception
        idt.invalid_tss.set_handler_fn(invalid_tss_handler);
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
    }

    idt_ref.load();
}


/// calls println!() and then println_raw!()
macro_rules! println_both {
    ($fmt:expr) => {
        print_raw!(concat!($fmt, "\n"));
        print!(concat!($fmt, "\n"));
    };
    ($fmt:expr, $($arg:tt)*) => {
        print_raw!(concat!($fmt, "\n"), $($arg)*);
        print!(concat!($fmt, "\n"), $($arg)*);
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
fn kill_and_halt(exception_number: u8, stack_frame: &ExceptionStackFrame) {
    #[cfg(unwind_exceptions)] {
        println_both!("Unwinding {:?} due to exception {}.", task::get_my_current_task(), exception_number);
    }
    #[cfg(not(unwind_exceptions))] {
        println_both!("Killing task without unwinding {:?} due to exception {}. (cfg `unwind_exceptions` is not set.)", task::get_my_current_task(), exception_number);
    }

    {
        let fe = fault_log::get_last_entry();
        if fe.is_some(){
            let fault_entry = fe.unwrap();
            let curr_task = task::get_my_current_task().expect("kill_and_halt: no current task");
            let namespace = curr_task.get_namespace();
            let task_name = {
                curr_task.lock().name.clone()
            };
            let app_crate :Option<String> = {
                let t = curr_task.lock();
                if t.app_crate.is_some(){
                    Some(t.app_crate.as_ref().unwrap().lock_as_ref().crate_name.clone())
                } else {
                    None
                }
                //t.app_crate.as_ref().expect("kill_and_halt: no app_crate").clone_shallow()
            };
            let instruction_pointer = VirtualAddress::new_canonical(stack_frame.instruction_pointer.0);
            let error_crate_name :Option<String> = match namespace.get_crate_containing_address(instruction_pointer.clone(),false){
                Some(cn) => {
                    Some(cn.lock_as_ref().crate_name.clone())
                }
                None => {
                    None
                }
            };

            let core = get_my_apic_id();

            add_error_to_fault_log (
                fault_entry.exception_number, //exception_number
                fault_entry.error_code, //error_code,
                core, //core
                task_name, //running_task
                app_crate, //running_app_crate: Option<None>,
                fault_entry.address_accessed, // address_accessed: Option<None>,
                Some(instruction_pointer), //instruction_pointer, //instruction_pointer : Option<None>,
                error_crate_name, //crate_error_occured, //crate_error_occured : Option<None>,
                fault_entry.replaced_crates, //replaced_crates : Vec<String>::new(),
                false // action_taken : false,
            );
        
            fault_log::print_fault_log();
        }
    }

    // Dump some info about the this loaded app crate
    // and test out using debug info for recovery
    if false {
        let curr_task = task::get_my_current_task().expect("kill_and_halt: no current task");
        let app_crate = {
            let t = curr_task.lock();
            t.app_crate.as_ref().expect("kill_and_halt: no app_crate").clone_shallow()
        };
        let debug_symbols_file = {
            let krate = app_crate.lock_as_ref();
            trace!("============== Crate {} =================", krate.crate_name);
            for s in krate.sections.values() {
                trace!("   {:?}", &*s);
            }
            krate.debug_symbols_file.clone()
        };

        if false {
            let mut debug = debug_info::DebugSymbols::Unloaded(debug_symbols_file);
            let debug_sections = debug.load(&app_crate, &curr_task.get_namespace()).unwrap();
            let instr_ptr = stack_frame.instruction_pointer.0 - 1; // points to the next instruction (at least for a page fault)

            let res = debug_sections.find_subprogram_containing(memory::VirtualAddress::new_canonical(instr_ptr));
            debug!("Result of find_subprogram_containing: {:?}", res);
        }
    }

    // print a stack trace
    println_both!("------------------ Stack Trace (DWARF) ---------------------------");
    let stack_trace_result = stack_trace::stack_trace(
        &|stack_frame, stack_frame_iter| {
            let symbol_offset = stack_frame_iter.namespace().get_section_containing_address(
                memory::VirtualAddress::new_canonical(stack_frame.call_site_address() as usize),
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

    let cause = task::KillReason::Exception(exception_number);

    // Unwind the current task that failed due to the given exception.
    // Currently this isn't working perfectly, so it's disabled by default.
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
        let res = task::get_my_current_task().ok_or("couldn't get current task").and_then(|taskref| taskref.kill(cause));
        match res {
            Ok(()) => { println_both!("Task {:?} killed itself successfully", task::get_my_current_task()); }
            Err(e) => { println_both!("Task {:?} was unable to kill itself. Error: {:?}", task::get_my_current_task(), e); }
        }
    }

    // If we failed to handle the exception and unwind the task, there's not really much we can do about it,
    // other than just let the thread spin endlessly (which doesn't hurt correctness but is inefficient). 
    // But in general, this task should have already been marked as killed and thus no longer schedulable,
    // so it should not reach this point. 
    // Only exceptions early on in the initialization process will get here, meaning that the OS will basically stop.
    loop { }
}



/// exception 0x00
pub extern "x86-interrupt" fn divide_by_zero_handler(stack_frame: &mut ExceptionStackFrame) {
    println_both!("\nEXCEPTION: DIVIDE BY ZERO\n{:#?}\n", stack_frame);

    add_error_simple(0x0, 0);
    kill_and_halt(0x0, stack_frame)
}

/// exception 0x01
pub extern "x86-interrupt" fn debug_handler(stack_frame: &mut ExceptionStackFrame) {
    println_both!("\nEXCEPTION: DEBUG at {:#X}\n{:#?}\n",
             stack_frame.instruction_pointer,
             stack_frame);

    // don't halt here, this isn't a fatal/permanent failure, just a brief pause.
}

/// exception 0x02, also used for TLB Shootdown IPIs and sampling interrupts
extern "x86-interrupt" fn nmi_handler(stack_frame: &mut ExceptionStackFrame) {
    let mut expected_nmi = false;
    
    // sampling interrupt handler: increments a counter, records the IP for the sample, and resets the hardware counter 
    if rdmsr(IA32_PERF_GLOBAL_STAUS) != 0 {
        pmu_x86::handle_sample(stack_frame);
        expected_nmi = true;
    }

    // currently we're using NMIs to send TLB shootdown IPIs
    let vaddrs = tlb_shootdown::TLB_SHOOTDOWN_IPI_VIRTUAL_ADDRESSES.read();
    if !vaddrs.is_empty() {
        // trace!("nmi_handler (AP {})", apic::get_my_apic_id().unwrap_or(0xFF));
        tlb_shootdown::handle_tlb_shootdown_ipi(&vaddrs);
        expected_nmi = true;
    }

    if expected_nmi {
        return;
    }

    println_both!("\nEXCEPTION: NON-MASKABLE INTERRUPT at {:#X}\n{:#?}\n",
             stack_frame.instruction_pointer,
             stack_frame);

    add_error_simple(0x02, 0);
    kill_and_halt(0x2, stack_frame)
}


/// exception 0x03
pub extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut ExceptionStackFrame) {
    println_both!("\nEXCEPTION: BREAKPOINT at {:#X}\n{:#?}\n",
             stack_frame.instruction_pointer,
             stack_frame);

    // don't halt here, this isn't a fatal/permanent failure, just a brief pause.
}

/// exception 0x04
pub extern "x86-interrupt" fn overflow_handler(stack_frame: &mut ExceptionStackFrame) {
    println_both!("\nEXCEPTION: OVERFLOW at {:#X}\n{:#?}\n",
             stack_frame.instruction_pointer,
             stack_frame);
    
    add_error_simple(0x04, 0);
    kill_and_halt(0x4, stack_frame)
}

// exception 0x05
pub extern "x86-interrupt" fn bound_range_exceeded_handler(stack_frame: &mut ExceptionStackFrame) {
    println_both!("\nEXCEPTION: BOUND RANGE EXCEEDED at {:#X}\n{:#?}\n",
             stack_frame.instruction_pointer,
             stack_frame);
    
    add_error_simple(0x05, 0);
    kill_and_halt(0x5, stack_frame)
}

/// exception 0x06
pub extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: &mut ExceptionStackFrame) {
    println_both!("\nEXCEPTION: INVALID OPCODE at {:#X}\n{:#?}\n",
             stack_frame.instruction_pointer,
             stack_frame);

    
    add_error_simple(0x06, 0);
    kill_and_halt(0x6, stack_frame)
}

/// exception 0x07
/// see this: http://wiki.osdev.org/I_Cant_Get_Interrupts_Working#I_keep_getting_an_IRQ7_for_no_apparent_reason
pub extern "x86-interrupt" fn device_not_available_handler(stack_frame: &mut ExceptionStackFrame) {
    println_both!("\nEXCEPTION: DEVICE_NOT_AVAILABLE at {:#X}\n{:#?}\n",
             stack_frame.instruction_pointer,
             stack_frame);

    add_error_simple(0x07, 0);
    kill_and_halt(0x7, stack_frame)
}

/// exception 0x08
pub extern "x86-interrupt" fn double_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: DOUBLE FAULT\n{:#?}\n", stack_frame);
    
    add_error_simple(0x08, error_code);
    kill_and_halt(0x8, stack_frame)
}

/// exception 0x0a
pub extern "x86-interrupt" fn invalid_tss_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: INVALID_TSS FAULT\nerror code: \
                                  {:#b}\n{:#?}\n",
             error_code,
             stack_frame);
    
    add_error_simple(0x0a, error_code);
    kill_and_halt(0xA, stack_frame)
}

/// exception 0x0b
pub extern "x86-interrupt" fn segment_not_present_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: SEGMENT_NOT_PRESENT FAULT\nerror code: \
                                  {:#b}\n{:#?}\n",
             error_code,
             stack_frame);
    
    add_error_simple(0x0b, error_code);
    kill_and_halt(0xB, stack_frame)
}

/// exception 0x0d
pub extern "x86-interrupt" fn general_protection_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    println_both!("\nEXCEPTION: GENERAL PROTECTION FAULT \nerror code: \
                                  {:#X}\n{:#?}\n",
             error_code,
             stack_frame);

    add_error_simple(0x0d, error_code);
    kill_and_halt(0xD, stack_frame)
}

/// exception 0x0e
pub extern "x86-interrupt" fn page_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: PageFaultErrorCode) {
    use x86_64::registers::control_regs;
    println_both!("\nEXCEPTION: PAGE FAULT while accessing {:#X}\nerror code: \
                                  {:?}\n{:#?}\n",
             control_regs::cr2(),
             error_code,
             stack_frame);
    
    let vec :Vec<String> = Vec::new();
    add_error_to_fault_log (
        0x0e, //exception_number
        0, //error_code,
        None, //core
        "Temporary".to_string(), //running_task
        None, //running_app_crate: Option<None>,
        Some(VirtualAddress::new_canonical(control_regs::cr2().0)), // address_accessed: Option<None>,
        None, //instruction_pointer : Option<None>,
        None, //crate_error_occured : Option<None>,
        vec, //replaced_crates : Vec<String>::new(),
        false // action_taken : false,
    );

    //fault_log::print_fault_log();



    kill_and_halt(0xE, stack_frame)
}

// exception 0x0F is reserved on x86
