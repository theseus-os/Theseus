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
use port_io::Port;
use drivers::input::keyboard;
use arch;



mod gdt;
pub mod pit_clock; // TODO: shouldn't be pub
mod pic;
mod time_tools; //testing whether including a module makes any difference
mod rtc;


const DOUBLE_FAULT_IST_INDEX: usize = 0;

lazy_static! {
    static ref IDT: Idt = {
        let mut idt = ::x86_64::structures::idt::Idt::new();

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
        // missing: 0x0d general protection exception
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


		// SET UP CUSTOM INTERRUPT HANDLERS
		// we can directly index the "idt" object because it implements the Index/IndexMut traits
        idt[0x20].set_handler_fn(timer_handler); // int 32
        idt[0x21].set_handler_fn(keyboard_handler); // int 33

        // TODO: add more 


        idt // return idt so it's set to the static ref IDT above
    };
}

/// Interface to our PIC (programmable interrupt controller) chips.
/// We want to map hardware interrupts to 0x20 (for PIC1) or 0x28 (for PIC2).
static mut PIC: pic::ChainedPics = unsafe { pic::ChainedPics::new(0x20, 0x28) };
static KEYBOARD: Mutex<Port<u8>> = Mutex::new(Port::new(0x60));

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

        PIC.initialize();
    }

    IDT.load();
    info!("loaded interrupt descriptor table.");

    // init PIT clock to 100 Hz
    pit_clock::init(100);
}



/// interrupt 0x00
extern "x86-interrupt" fn divide_by_zero_handler(stack_frame: &mut ExceptionStackFrame) {
    println!("\nEXCEPTION: DIVIDE BY ZERO\n{:#?}", stack_frame);
    loop {}
}

/// interrupt 0x03
extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut ExceptionStackFrame) {
    println!("\nEXCEPTION: BREAKPOINT at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
}

/// interrupt 0x06
extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: &mut ExceptionStackFrame) {
    println!("\nEXCEPTION: INVALID OPCODE at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
    loop {}
}

/// interrupt 0x07
/// see this: http://wiki.osdev.org/I_Cant_Get_Interrupts_Working#I_keep_getting_an_IRQ7_for_no_apparent_reason
extern "x86-interrupt" fn device_not_available_handler(stack_frame: &mut ExceptionStackFrame) {
    println!("\nEXCEPTION: DEVICE_NOT_AVAILABLE at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);

	// TODO: handle this
	/* When any IRQ7 is received, simply read the In-Service Register
		 outb(0x20, 0x0B); unsigned char irr = inb(0x20);
		and check if bit 7
		irr & 0x80
		is set. If it isn't, then return from the interrupt without sending an EOI.
	*/
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



/// this shouldn't really ever happen, but I added the handler anyway
/// because I noticed the interrupt 0xb happening when other interrupts weren't properly handled
extern "x86-interrupt" fn segment_not_present_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    use x86_64::registers::control_regs;
    println!("\nEXCEPTION: SEGMENT_NOT_PRESENT FAULT\nerror code: \
                                  {:#b}\n{:#?}",
//             control_regs::cr2(),
             error_code,
             stack_frame);

    loop {}
}

// 0x20
extern "x86-interrupt" fn timer_handler(stack_frame: &mut ExceptionStackFrame) {
    ::drivers::serial_port::serial_out("\n\x1b[33m[W] TIMER! \x1b[0m\n");

    // we must acknowledge the interrupt first before handling it, which will cause a context switch
	unsafe { PIC.notify_end_of_interrupt(0x20); }
    //time_tools::return_ticks();

    pit_clock::handle_timer_interrupt();
}


// 0x21
extern "x86-interrupt" fn keyboard_handler(stack_frame: &mut ExceptionStackFrame) {
    // in this interrupt, we must read the keyboard scancode register before acknowledging the interrupt.
    let mut scan_code: u8 = { 
        KEYBOARD.lock().read() 
    };
	// trace!("KBD: {:?}", scan_code);


    keyboard::handle_keyboard_input(scan_code);	
    unsafe { PIC.notify_end_of_interrupt(0x21); }
    
}


extern "x86-interrupt" fn unimplemented_interrupt_handler(stack_frame: &mut ExceptionStackFrame) {
	error!("caught unhandled interrupt: {:#?}", stack_frame);

}










/// A handle for frozen interrupts
#[derive(Default)]
pub struct HeldInterrupts(bool);

/// Prevent interrupts from firing until return value is dropped (goes out of scope). 
/// After it is dropped, the interrupts are returned to their prior state, not blindly re-enabled. 
pub fn hold_interrupts() -> HeldInterrupts {
    let enabled = interrupts_enabled();
	let retval = HeldInterrupts(enabled);
    disable_interrupts();
    // trace!("hold_interrupts(): disabled interrupts, were {}", enabled);
    retval
}


impl ::core::ops::Drop for HeldInterrupts {
	fn drop(&mut self)
	{
        // trace!("hold_interrupts(): enabling interrupts? {}", self.0);
		if self.0 {
			enable_interrupts();
			// unsafe { asm!("sti" : : : "memory" : "volatile"); }
		}
	}
}


/// disable interrupts
pub fn disable_interrupts() {
    arch::disable_interrupts();
}

/// enable interrupts
pub fn enable_interrupts() {
    arch::enable_interrupts();
}

/// returns true if interrupts are currently enabled
pub fn interrupts_enabled() -> bool {
    arch::interrupts_enabled()
}
