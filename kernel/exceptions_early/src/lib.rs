//! Early exception handlers that do nothing but print an error and hang.

#![no_std]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate vga_buffer; // for println_raw!()
extern crate x86_64;
extern crate mod_mgmt;
extern crate memory; 
extern crate spin;
extern crate tss;
extern crate gdt;

use spin::Mutex;
use x86_64::structures::{
    idt::{LockedIdt, ExceptionStackFrame, PageFaultErrorCode},
    tss::TaskStateSegment,
};
use gdt::{Gdt, create_gdt};

/// The initial GDT structure for the BSP (the first CPU to boot),
/// which is only really used for the purpose of setting up a special
/// interrupt stack to support gracefully handling early double faults. 
static EARLY_GDT: Mutex<Gdt> = Mutex::new(Gdt::new());

/// The initial TSS structure for the BSP (the first CPU to boot),
/// which is only really used for the purpose of setting up a special
/// interrupt stack to support gracefully handling early double faults. 
static EARLY_TSS: Mutex<TaskStateSegment> = Mutex::new(TaskStateSegment::new());

/// Initializes the given `IDT` with a basic set of early exception handlers
/// that print out basic information when an exception occurs, mostly for debugging.
///
/// If a double fault stack address is specified, a new TSS and GDT
/// will be created and set up such that the processor will jump to 
/// that stack upon a double fault.
pub fn init(
    idt_ref: &'static LockedIdt, 
    double_fault_stack_top_unusable: Option<memory::VirtualAddress>,
) {
    println_raw!("exceptions_early(): double_fault_stack_top_unusable: {:X?}", double_fault_stack_top_unusable);
    if let Some(df_stack_top) = double_fault_stack_top_unusable {
        // Create and load an initial TSS and GDT so we can handle early exceptions such as double faults. 
        let mut tss = TaskStateSegment::new();
        tss.interrupt_stack_table[tss::DOUBLE_FAULT_IST_INDEX] = x86_64::VirtualAddress(df_stack_top.value());
        println_raw!("exceptions_early(): Created TSS: {:?}", tss);
        *EARLY_TSS.lock() = tss;
        
        let (gdt, kernel_cs, kernel_ds, _user_cs_32, _user_ds_32, _user_cs_64, _user_ds_64, tss_segment) = create_gdt(&*EARLY_TSS.lock());
        *EARLY_GDT.lock() = gdt;
        EARLY_GDT.lock().load();

        use x86_64::instructions::{
            segmentation::{set_cs, load_ds, load_ss},
            tables::load_tss,
        };

        unsafe {
            set_cs(kernel_cs); // reload code segment register
            load_tss(tss_segment);      // load TSS
            let kernel_ds_2 = x86_64::structures::gdt::SegmentSelector::new(kernel_ds.index(), kernel_ds.rpl());
            load_ss(kernel_ds); // unsure if necessary, but doesn't hurt
            load_ds(kernel_ds_2); // unsure if necessary, but doesn't hurt
        }
    }

    { 
        let mut idt = idt_ref.lock(); // withholds interrupts

        // SET UP FIXED EXCEPTION HANDLERS
        idt.divide_by_zero.set_handler_fn(divide_by_zero_handler);
        // missing: 0x01 debug exception
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        // missing: 0x04 overflow exception
        // missing: 0x05 bound range exceeded exception
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.device_not_available.set_handler_fn(device_not_available_handler);
        let double_fault_idt_entry_options = idt.double_fault.set_handler_fn(double_fault_handler);
        if let Some(_df_stack_top) = double_fault_stack_top_unusable {
            // SAFE: we set up the required TSS index at the top of this function.
            unsafe {
                double_fault_idt_entry_options.set_stack_index(tss::DOUBLE_FAULT_IST_INDEX as u16);
            }
        }

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

    idt_ref.load();
}




/// exception 0x00
pub extern "x86-interrupt" fn divide_by_zero_handler(stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nEXCEPTION (early): DIVIDE BY ZERO\n{:#?}", stack_frame);

    loop {}
}



/// exception 0x02
pub extern "x86-interrupt" fn nmi_handler(stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nEXCEPTION (early): NON-MASKABLE INTERRUPT at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);
    
    loop { }
}


/// exception 0x03
pub extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nEXCEPTION (early): BREAKPOINT at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);

    // don't halt here, this isn't a fatal/permanent failure, just a brief pause.
}

/// exception 0x06
pub extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nEXCEPTION (early): INVALID OPCODE at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);

    loop {}
}

/// exception 0x07
/// 
/// For more information about "spurious interrupts", 
/// see [here](http://wiki.osdev.org/I_Cant_Get_Interrupts_Working#I_keep_getting_an_IRQ7_for_no_apparent_reason).
pub extern "x86-interrupt" fn device_not_available_handler(stack_frame: &mut ExceptionStackFrame) {
    println_raw!("\nEXCEPTION (early): DEVICE_NOT_AVAILABLE at {:#x}\n{:#?}",
             stack_frame.instruction_pointer,
             stack_frame);

    loop {}
}


pub extern "x86-interrupt" fn double_fault_handler(stack_frame: &mut ExceptionStackFrame, _error_code: u64) {
    println_raw!("\nEXCEPTION (early): DOUBLE FAULT\n{:#?}", stack_frame);
    println_raw!("\nNote: this may be caused by stack overflow. Is the size of the initial_bsp_stack is too small?");

    loop {}
}


pub extern "x86-interrupt" fn segment_not_present_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION (early): SEGMENT_NOT_PRESENT FAULT\nerror code: \
                                  {:#b}\n{:#?}",
             error_code,
             stack_frame);

    loop {}
}


pub extern "x86-interrupt" fn general_protection_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION (early): GENERAL PROTECTION FAULT \nerror code: \
                                  {:#X}\n{:#?}",
             error_code,
             stack_frame);

    loop {}
}


pub extern "x86-interrupt" fn early_page_fault_handler(stack_frame: &mut ExceptionStackFrame, error_code: PageFaultErrorCode) {
    use x86_64::registers::control_regs;
    let accessed_address = control_regs::cr2();
    println_raw!("\nEXCEPTION (early): PAGE FAULT (early handler) while accessing {:#x}\nerror code: \
        {:?}\n{:#?}",
        accessed_address,
        error_code,
        stack_frame
    );

    println_raw!("Exception IP {:#X} is at {:?}", 
        stack_frame.instruction_pointer, 
        mod_mgmt::get_initial_kernel_namespace().and_then(|ns| ns.get_section_containing_address(
            memory::VirtualAddress::new_canonical(stack_frame.instruction_pointer.0 as usize),
            false // only look at .text sections, not all other types
        )),
    );
    println_raw!("Faulted access address {:#X} is at {:?}",
        accessed_address,
        mod_mgmt::get_initial_kernel_namespace().and_then(|ns| ns.get_section_containing_address(
            memory::VirtualAddress::new_canonical(accessed_address.0 as usize),
            true, // look at all sections (.data/.bss/.rodata), not just .text
        )),
    );
    loop {}
}
