// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use memory::MemoryController;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::idt::{Idt, ExceptionStackFrame, PageFaultErrorCode};
use spin::{Mutex, Once};


mod gdt;
mod pic;

const DOUBLE_FAULT_IST_INDEX: usize = 0;

lazy_static! {
    static ref IDT: Idt = {
        let mut idt = ::x86_64::structures::idt::Idt::new();

		// set up exceptions
        idt.divide_by_zero.set_handler_fn(divide_by_zero_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        
        
        // fill all IDT entries with an unimplemented IRQ handler
        //FIXME:  this should be from 32..255, but 224 doesn't work for some reason
        for i in 32..224 {
	        idt.interrupts[i].set_handler_fn(unimplemented_interrupt_handler);
//	        println!("set interrupt {}", i);
        }
        
        // set our custom interrupts 
        idt.interrupts[0x0 /* 0x20 */].set_handler_fn(timer_handler); // int 32
        idt.interrupts[0x1 /* 0x21 */].set_handler_fn(keyboard_handler); // int 33
	
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(DOUBLE_FAULT_IST_INDEX as u16); // use a special stack for the DF handler
        }

        idt
    };
}

/// Interface to our PIC (programmable interrupt controller) chips.  
/// We want to map hardware interrupts to 0x20 (for PIC1) or 0x28 (for PIC2).
static PIC: Mutex<pic::ChainedPics> = Mutex::new(unsafe { pic::ChainedPics::new(0x20, 0x28) });

static TSS: Once<TaskStateSegment> = Once::new();
static GDT: Once<gdt::Gdt> = Once::new();


pub fn init(memory_controller: &mut MemoryController) {
    use x86_64::structures::gdt::SegmentSelector;
    use x86_64::instructions::segmentation::set_cs;
    use x86_64::instructions::tables::load_tss;
    use x86_64::VirtualAddress;

    let double_fault_stack =
        memory_controller.alloc_stack(1).expect("could not allocate double fault stack");

    let tss = TSS.call_once(|| {
                                let mut tss = TaskStateSegment::new();
                                tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX] = VirtualAddress(double_fault_stack.top());
                                tss
                            });

    let mut code_selector = SegmentSelector(0);
    let mut tss_selector = SegmentSelector(0);
    let gdt = GDT.call_once(|| {
        let mut gdt = gdt::Gdt::new();
        code_selector = gdt.add_entry(gdt::Descriptor::kernel_code_segment());
        tss_selector = gdt.add_entry(gdt::Descriptor::tss_segment(&tss));
        gdt
    });
    gdt.load();

    unsafe {
        set_cs(code_selector); // reload code segment register
        load_tss(tss_selector); // load TSS
        
	    PIC.lock().initialize();
    }

    IDT.load();
    

}


//macro_rules! define_unimplemented_interrupt_handler {
//	($num: expr) => ( 
//		extern "x86-interrupt" fn stringify!($num)(stack_frame: &mut ExceptionStackFrame) {
//			println!("caught unhandled interrupt {}: {:#?}", $num, stack_frame);
//		}
//	);
//}
//
//
//define_unimplemented_interrupt_handler!(32, );
//
//
//
//define_all_interrupt_handlers!();


extern "x86-interrupt" fn divide_by_zero_handler(stack_frame: &mut ExceptionStackFrame) {
    println!("\nEXCEPTION: DIVIDE BY ZERO\n{:#?}", stack_frame);
    loop {}
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut ExceptionStackFrame) {
    println!("\nEXCEPTION: BREAKPOINT at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: &mut ExceptionStackFrame) {
    println!("\nEXCEPTION: INVALID OPCODE at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
    loop {}
}

extern "x86-interrupt" fn page_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: PageFaultErrorCode) {
    use x86_64::registers::control_regs;
    println!("\nEXCEPTION: PAGE FAULT while accessing {:#x}\nerror code: \
                                  {:?}\n{:#?}",
             control_regs::cr2(),
             error_code,
             stack_frame);
    loop {}
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: &mut ExceptionStackFrame, _error_code: u64) {
    println!("\nEXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
    loop {}
}




extern "x86-interrupt" fn segment_not_present_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    use x86_64::registers::control_regs;
    println!("\nEXCEPTION: SEGMENT_NOT_PRESENT FAULT\nerror code: \
                                  {:#b}\n{:#?}",
//             control_regs::cr2(),
             error_code,
             stack_frame);
    
    loop {}
}


extern "x86-interrupt" fn timer_handler(stack_frame: &mut ExceptionStackFrame) {
//	println!("\nTIMER interrupt:\n{:#?}", stack_frame);
	unsafe { PIC.lock().notify_end_of_interrupt(0x20u8); }
}


extern "x86-interrupt" fn keyboard_handler(stack_frame: &mut ExceptionStackFrame) {
	println!("\nKEYBOARD interrupt:\n{:#?}", stack_frame);
	unsafe { PIC.lock().notify_end_of_interrupt(0x21u8); }
}


extern "x86-interrupt" fn unimplemented_interrupt_handler(stack_frame: &mut ExceptionStackFrame) {
	println!("caught unhandled interrupt: {:#?}", stack_frame);
	
}



///////////////////////////////////////////////////////////////////////////
///////////////////////////////////////////////////////////////////////////
//////////////////////        from toyos             //////////////////////
///////////////////////////////////////////////////////////////////////////

/*

/// Various data available on our stack when handling an interrupt.
///
/// Only `pub` because `rust_interrupt_handler` is.
#[repr(C, packed)]
pub struct InterruptContext {
    rsi: u64,
    rdi: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rdx: u64,
    rcx: u64,
    rax: u64,
    int_id: u32,
    _pad_1: u32,
    error_code: u32,
    _pad_2: u32,
}


/// Print our information about a CPU exception, and loop.
fn cpu_exception_handler(ctx: &InterruptContext) {

    // Print general information provided by x86::irq.
    println!("{}, error 0x{:x}",
             x86::irq::EXCEPTIONS[ctx.int_id as usize],
             ctx.error_code);

    // Provide detailed information about our error code if we know how to
    // parse it.
    match ctx.int_id {
        14 => {
            let err = x86::irq::PageFaultError::from_bits(ctx.error_code);
            println!("{:?}", err);
        }
        _ => {}
    }

    loop {}
}

/// Called from our assembly-language interrupt handlers to dispatch an
/// interrupt.
#[no_mangle]
pub unsafe extern "C" fn rust_interrupt_handler(ctx: &InterruptContext) {
    match ctx.int_id {
        0x00...0x0F => cpu_exception_handler(ctx),
        0x20 => { } // timer
        0x21 => {
            if let Some(input) = keyboard::read_char() {
                if input == '\r' {
                    println!("");
                } else {
                    print!("{}", input);
                }
            }
        }
        0x80 => println!("Not actually Linux, sorry."),
        _ => {
            println!("UNKNOWN INTERRUPT #{}", ctx.int_id);
            loop {}
        }
    }

    PICS.lock().notify_end_of_interrupt(ctx.int_id as u8);
}





//=========================================================================
//  Initialization

/// Use the `int` instruction to manually trigger an interrupt without
/// actually using `sti` to enable interrupts.  This is highly recommended by
/// http://jvns.ca/blog/2013/12/04/day-37-how-a-keyboard-works/
#[allow(dead_code)]
pub unsafe fn test_interrupt() {
    println!("Triggering interrupt.");
    int!(0x80);
    println!("Interrupt returned!");
}

/// Platform-independent initialization.
pub unsafe fn initialize() {
    PICS.lock().initialize();
    IDT.lock().initialize();

    // Enable this to trigger a sample interrupt.
    test_interrupt();

    // Turn on real interrupts.
    x86::irq::enable();
}

*/