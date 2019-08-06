//! Early exception handlers that do nothing but print an error and hang.

#![no_std]
#![feature(asm)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;
extern crate spin;

use spin::Mutex;

lazy_static! {
    static ref VECTORS: Mutex<[ExceptionEntry; 16]> = Mutex::new([ExceptionEntry::new(); 16]);
}

#[cfg(target_arch = "aarch64")]
/// Initialize the exception vectors
/// In arm, the address of the vector is in VBAR_EL1. The vector contains 16 entries and their addresses are aligned with 0x80. Every entry is the handler of a type of exception. Once an exception occurs, the system jumps to the address of the related entry and continues with the handler. Usually we do not put handlers in the entries directly, but put `bl handler` in the entry so that the system can jump to a custom handler.
pub fn init() {
    let mut exceptions = VECTORS.lock();

    exceptions[0].instructions[0] = assemble_inst_bl(&exceptions[0], exception_synchronous_handler);
    exceptions[1].instructions[0] = assemble_inst_bl(&exceptions[1], exception_irq_handler);
    exceptions[2].instructions[0] = assemble_inst_bl(&exceptions[2], exception_fiq_handler);
    exceptions[3].instructions[0] = assemble_inst_bl(&exceptions[3], exception_serror_handler);
    exceptions[4].instructions[0] = assemble_inst_bl(&exceptions[4], exception_synchronous_handler);
    exceptions[5].instructions[0] = assemble_inst_bl(&exceptions[5], exception_irq_handler);
    exceptions[6].instructions[0] = assemble_inst_bl(&exceptions[6], exception_fiq_handler);
    exceptions[7].instructions[0] = assemble_inst_bl(&exceptions[7], exception_serror_handler);
    exceptions[8].instructions[0] = assemble_inst_bl(&exceptions[8], exception_synchronous_handler);
    exceptions[9].instructions[0] = assemble_inst_bl(&exceptions[9], exception_irq_handler);
    exceptions[10].instructions[0] = assemble_inst_bl(&exceptions[10], exception_fiq_handler);
    exceptions[11].instructions[0] = assemble_inst_bl(&exceptions[11], exception_serror_handler);
    exceptions[12].instructions[0] =
        assemble_inst_bl(&exceptions[12], exception_synchronous_handler);
    exceptions[13].instructions[0] = assemble_inst_bl(&exceptions[13], exception_irq_handler);
    exceptions[14].instructions[0] = assemble_inst_bl(&exceptions[14], exception_fiq_handler);
    exceptions[15].instructions[0] = assemble_inst_bl(&exceptions[15], exception_serror_handler);

    let address = exceptions.as_ptr() as u32;
    unsafe {
        asm!("
            msr VBAR_EL1, x0;
            dsb ish; 
            isb; " : :"{x0}"(address): : "volatile");
    }
}

// Assembles a `bl handler` instruction. Passes the address of the vector because the address of the destination is relative
fn assemble_inst_bl(vector: &ExceptionEntry, handler: fn()) -> u32 {
    const INST_BL: u32 = 0x94000000;
    const BL_RANGE: u32 = 0x4000000;
    let dest_addr = handler as u32;
    let src_addr = vector as *const _ as u32;
    let offset;
    if src_addr > dest_addr {
        offset = BL_RANGE - (src_addr as u32 - dest_addr as u32) / 4;
    } else {
        offset = dest_addr - src_addr;
    }
    INST_BL + offset
}

#[derive(Copy, Clone)]
#[repr(C, align(0x80))]
struct ExceptionEntry {
    instructions: [u32; 0x20],
}

impl ExceptionEntry {
    fn new() -> ExceptionEntry {
        ExceptionEntry {
            instructions: [0; 0x20],
        }
    }
}

// fn _exception_default_handler() {
//     error!("Default Exceptions!");
//     print_registers();
//     loop {}
// }

// The exception vection contains 16 entries. 
// These entries represent four different types of exceptions at four different level. 
// Here we define four exceptions for each exception type and print the corresponding registers. 
// In the future, we will handle exceptions such as interrupts and print more information.
fn exception_synchronous_handler() {
    error!("Synchronous Exceptions!");
    print_registers();
    loop {}
}

fn exception_irq_handler() {
    error!("IRQ Exceptions!");
    print_registers();
    loop {}
}

fn exception_fiq_handler() {
    error!("FIQ Exceptions!");
    print_registers();
    loop {}
}

fn exception_serror_handler() {
    error!("SErrorExceptions!");
    print_registers();
    loop {}
}

fn print_registers() {
    let esr_el1: u64;
    let elr_el1: u64;
    let spsr_el1: u64;
    let far_el1: u64;
    unsafe {
        asm!("mrs $0, ESR_EL1" : "=r"(esr_el1) : : : "volatile");
        asm!("mrs $0, ELR_EL1" : "=r"(elr_el1) : : : "volatile");
        asm!("mrs $0, SPSR_EL1" : "=r"(spsr_el1) : : : "volatile");
        asm!("mrs $0, FAR_EL1" : "=r"(far_el1) : : : "volatile");
    };
    error!(
        "ESR_EL1={:X}\nELR_EL1={:X}\nSPSR_EL1={:X}\nFAR_EL1={:X}\n",
        esr_el1, elr_el1, spsr_el1, far_el1
    );
}
