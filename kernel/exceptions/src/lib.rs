#![no_std]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
#[macro_use] extern crate vga_buffer;
extern crate x86_64;


use x86_64::structures::idt::{LockedIdt, ExceptionStackFrame, PageFaultErrorCode};


pub fn init_early_exceptions(early_idt: &'static LockedIdt) {
    { 
        let mut idt = early_idt.lock(); // withholds interrupts

        // SET UP FIXED EXCEPTION HANDLERS
        idt.divide_by_zero.set_handler_fn(divide_by_zero_handler);
        // missing: 0x01 debug exception
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        // missing: 0x04 overflow exception
        // missing: 0x05 bound range exceeded exception
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.device_not_available.set_handler_fn(device_not_available_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        // reserved: 0x09 coprocessor segment overrun exception
        // missing: 0x0a invalid TSS exception
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        // missing: 0x0c stack segment exception
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.page_fault.set_handler_fn(early_page_fault_handler);
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

    early_idt.load();
    info!("loaded early IDT with basic exception handlers only.");
}




/// exception 0x00
pub extern "x86-interrupt" fn divide_by_zero_handler(stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nEXCEPTION: DIVIDE BY ZERO\n{:#?}", stack_frame);
    loop {}
}



/// exception 0x02
pub extern "x86-interrupt" fn nmi_handler(stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nEXCEPTION: NON-MASKABLE INTERRUPT at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
    
    loop { }
}


/// exception 0x03
pub extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nEXCEPTION: BREAKPOINT at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
}

/// exception 0x06
pub extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nEXCEPTION: INVALID OPCODE at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
    loop {}
}

/// exception 0x07
/// see this: http://wiki.osdev.org/I_Cant_Get_Interrupts_Working#I_keep_getting_an_IRQ7_for_no_apparent_reason
pub extern "x86-interrupt" fn device_not_available_handler(stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nEXCEPTION: DEVICE_NOT_AVAILABLE at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);

    loop {}
}


pub extern "x86-interrupt" fn early_page_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: PageFaultErrorCode) {
    use x86_64::registers::control_regs;
    error!("\nEXCEPTION: PAGE FAULT (early handler) while accessing {:#x}\nerror code: \
                                  {:?}\n{:#?}",
             control_regs::cr2(),
             error_code,
             stack_frame);
    loop {}
}


pub extern "x86-interrupt" fn page_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: PageFaultErrorCode) {
    use x86_64::registers::control_regs;
    println_raw!("\nEXCEPTION: PAGE FAULT while accessing {:#x}\nerror code: \
                                  {:?}\n{:#?}",
             control_regs::cr2(),
             error_code,
             stack_frame);
    loop {}
}

pub extern "x86-interrupt" fn double_fault_handler(stack_frame: &mut ExceptionStackFrame, _error_code: u64) {
    println_raw!("\nEXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
    loop {}
}



/// this shouldn't really ever happen, but I added the handler anyway
/// because I noticed the exception 0xb happening when other interrupts weren't properly handled
pub extern "x86-interrupt" fn segment_not_present_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION: SEGMENT_NOT_PRESENT FAULT\nerror code: \
                                  {:#b}\n{:#?}",
             error_code,
             stack_frame);

    loop {}
}


pub extern "x86-interrupt" fn general_protection_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION: GENERAL PROTECTION FAULT \nerror code: \
                                  {:#X}\n{:#?}",
             error_code,
             stack_frame);


    // TODO: kill the offending process
    loop {}
}
