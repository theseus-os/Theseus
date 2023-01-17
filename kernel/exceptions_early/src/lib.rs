//! Early exception handlers that do nothing but print an error and hang.

// TODO: Add direct explanation to why each empty loop is necessary and criteria for replacing it with something else
#![allow(clippy::empty_loop)]
#![no_std]
#![feature(abi_x86_interrupt)]

use spin::Mutex;
use x86_64::{
    structures::{
        idt::{InterruptStackFrame, PageFaultErrorCode},
        tss::TaskStateSegment,
    },
    instructions::{
        segmentation::{CS, DS, SS, Segment}, 
        tables::load_tss,
    },
};
use locked_idt::LockedIdt;
use gdt::{Gdt, create_gdt};
use vga_buffer::println_raw;

/// An initial Interrupt Descriptor Table (IDT) with only very simple CPU exceptions handlers.
/// This is no longer used after interrupts are set up properly, it's just a failsafe.
pub static EARLY_IDT: LockedIdt = LockedIdt::new();

/// The initial GDT structure for the BSP (the first CPU to boot),
/// which is only really used for the purpose of setting up a special
/// interrupt stack to support gracefully handling early double faults. 
static EARLY_GDT: Mutex<Gdt> = Mutex::new(Gdt::new());

/// The initial TSS structure for the BSP (the first CPU to boot),
/// which is only really used for the purpose of setting up a special
/// interrupt stack to support gracefully handling early double faults. 
static EARLY_TSS: Mutex<TaskStateSegment> = Mutex::new(TaskStateSegment::new());

/// Initializes an early IDT with a basic set of early exception handlers
/// that print out basic information when an exception occurs, mostly for debugging.
///
/// If a double fault stack address is specified, a new TSS and GDT
/// will be created and set up such that the processor will jump to 
/// that stack upon a double fault.
pub fn init(double_fault_stack_top_unusable: Option<memory::VirtualAddress>) {
    println_raw!("exceptions_early(): double_fault_stack_top_unusable: {:X?}", double_fault_stack_top_unusable);
    if let Some(df_stack_top) = double_fault_stack_top_unusable {
        // Create and load an initial TSS and GDT so we can handle early exceptions such as double faults. 
        let mut tss = TaskStateSegment::new();
        tss.interrupt_stack_table[tss::DOUBLE_FAULT_IST_INDEX] = x86_64::VirtAddr::new(df_stack_top.value() as u64);
        println_raw!("exceptions_early(): Created TSS: {:?}", tss);
        *EARLY_TSS.lock() = tss;
        
        let (gdt, kernel_cs, kernel_ds, _user_cs_32, _user_ds_32, _user_cs_64, _user_ds_64, tss_segment) = create_gdt(&EARLY_TSS.lock());
        *EARLY_GDT.lock() = gdt;
        EARLY_GDT.lock().load();

        unsafe {
            CS::set_reg(kernel_cs);          // reload code segment register
            load_tss(tss_segment);           // load TSS
            SS::set_reg(kernel_ds);          // unsure if necessary, but doesn't hurt
            DS::set_reg(kernel_ds);          // unsure if necessary, but doesn't hurt
        }
    }

    { 
        let mut idt = EARLY_IDT.lock(); // withholds interrupts

        // SET UP FIXED EXCEPTION HANDLERS
        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.debug.set_handler_fn(debug_handler);
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.overflow.set_handler_fn(overflow_handler);
        idt.bound_range_exceeded.set_handler_fn(bound_range_exceeded_handler);
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
        idt.invalid_tss.set_handler_fn(invalid_tss_handler);
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.page_fault.set_handler_fn(early_page_fault_handler);
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

    EARLY_IDT.load();
}


/// exception 0x00
extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): DIVIDE ERROR\n{:#X?}", stack_frame);
    loop {}
}

/// exception 0x01
extern "x86-interrupt" fn debug_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): DEBUG EXCEPTION\n{:#X?}", stack_frame);
    // don't halt here, this isn't a fatal/permanent failure, just a brief pause.
}

/// exception 0x02
extern "x86-interrupt" fn nmi_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): NON-MASKABLE INTERRUPT\n{:#X?}", stack_frame);
    loop { }
}

/// exception 0x03
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): BREAKPOINT\n{:#X?}", stack_frame);
    // don't halt here, this isn't a fatal/permanent failure, just a brief pause.
}

/// exception 0x04
extern "x86-interrupt" fn overflow_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): OVERFLOW\n{:#X?}", stack_frame);
    loop { }
}

/// exception 0x05
extern "x86-interrupt" fn bound_range_exceeded_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): BOUND RANGE EXCEEDED\n{:#X?}", stack_frame);
    loop { }
}

/// exception 0x06
extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): INVALID OPCODE\n{:#X?}", stack_frame);
    loop {}
}

/// exception 0x07
/// 
/// For more information about "spurious interrupts", 
/// see [here](http://wiki.osdev.org/I_Cant_Get_Interrupts_Working#I_keep_getting_an_IRQ7_for_no_apparent_reason).
extern "x86-interrupt" fn device_not_available_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): DEVICE NOT AVAILABLE\n{:#X?}", stack_frame);
    loop {}
}

/// exception 0x08
/// 
/// Note: this is `pub` so we can access it within `interrupts::init()`.
pub extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) -> ! {
    println_raw!("\nEXCEPTION (early): DOUBLE FAULT\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    println_raw!("\nNote: this may be caused by stack overflow. Is the size of the initial_bsp_stack is too small?");
    loop {}
}

/// exception 0x0A
extern "x86-interrupt" fn invalid_tss_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION (early): INVALID TSS\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    loop {}
}

/// exception 0x0B
extern "x86-interrupt" fn segment_not_present_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION (early): SEGMENT NOT PRESENT\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    loop {}
}

/// exception 0x0C
extern "x86-interrupt" fn stack_segment_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION (early): STACK SEGMENT FAULT\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    loop {}
}

/// exception 0x0D
extern "x86-interrupt" fn general_protection_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION (early): GENERAL PROTECTION FAULT\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    loop {}
}

/// exception 0x0E
extern "x86-interrupt" fn early_page_fault_handler(stack_frame: InterruptStackFrame, error_code: PageFaultErrorCode) {
    let accessed_address = x86_64::registers::control::Cr2::read_raw();
    println_raw!("\nEXCEPTION (early): PAGE FAULT (early handler) while accessing {:#x}\n\
        error code: {:?}\n{:#X?}",
        accessed_address,
        error_code,
        stack_frame
    );

    println_raw!("Exception IP {:#X} is at {:?}", 
        stack_frame.instruction_pointer, 
        mod_mgmt::get_initial_kernel_namespace().and_then(|ns| ns.get_section_containing_address(
            memory::VirtualAddress::new_canonical(stack_frame.instruction_pointer.as_u64() as usize),
            false // only look at .text sections, not all other types
        )),
    );
    println_raw!("Faulted access address {:#X} is at {:?}",
        accessed_address,
        mod_mgmt::get_initial_kernel_namespace().and_then(|ns| ns.get_section_containing_address(
            memory::VirtualAddress::new_canonical(accessed_address as usize),
            true, // look at all sections (.data/.bss/.rodata), not just .text
        )),
    );
    loop {}
}

/// exception 0x10
extern "x86-interrupt" fn x87_floating_point_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): x87 FLOATING POINT\n{:#X?}", stack_frame);
    loop {}
}

/// exception 0x11
extern "x86-interrupt" fn alignment_check_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION (early): ALIGNMENT CHECK\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    loop {}
}

/// exception 0x12
extern "x86-interrupt" fn machine_check_handler(stack_frame: InterruptStackFrame) -> ! {
    println_raw!("\nEXCEPTION (early): MACHINE CHECK\n{:#X?}", stack_frame);
    loop {}
}

/// exception 0x13
extern "x86-interrupt" fn simd_floating_point_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): SIMD FLOATING POINT\n{:#X?}", stack_frame);
    loop {}
}

/// exception 0x14
extern "x86-interrupt" fn virtualization_handler(stack_frame: InterruptStackFrame) {
    println_raw!("\nEXCEPTION (early): VIRTUALIZATION\n{:#X?}", stack_frame);
    loop {}
}

/// exception 0x1D
extern "x86-interrupt" fn vmm_communication_exception_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION (early): VMM COMMUNICATION EXCEPTION\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    loop {}
}

/// exception 0x1E
extern "x86-interrupt" fn security_exception_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    println_raw!("\nEXCEPTION (early): SECURITY EXCEPTION\n{:#X?}\nError code: {:#b}", stack_frame, error_code);
    loop {}
}
