//! Early exception handlers that do nothing but print an error and hang.

#![no_std]
#![feature(asm)]

#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate spin;

use spin::Mutex;

lazy_static! {
    static ref VECTORS: Mutex<[ExceptionEntry; 16]> = Mutex::new([ExceptionEntry::new(); 16]);
}

#[cfg(any(target_arch="aarch64"))]
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
    exceptions[12].instructions[0] = assemble_inst_bl(&exceptions[12], exception_synchronous_handler);
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

fn assemble_inst_bl(vector:&ExceptionEntry, handler: fn()) -> u32 {
    const INST_BL:u32 = 0x94000000;
    const BL_RANGE:u32 = 0x4000000;
    let dest_addr = handler as u32;
    let src_addr = vector as *const _ as u32;
    let offset = BL_RANGE - (src_addr as u32 - dest_addr as u32)/4;
    INST_BL + offset
}

#[derive(Copy, Clone)]
#[repr(C, align(0x80))]
struct ExceptionEntry {
    instructions:[u32; 0x20]
}

impl ExceptionEntry {
    fn new() -> ExceptionEntry {
        ExceptionEntry {
            instructions:[0; 0x20]  
        }
    }
}


// TODO print register information
#[cfg(any(target_arch="aarch64"))]
fn _exception_default_handler() {
    error!("Default Exceptions!");
    loop { }
}

#[cfg(any(target_arch="aarch64"))]
fn exception_synchronous_handler() {
    error!("Synchronous Exceptions!");
    loop { }
}

#[cfg(any(target_arch="aarch64"))]
fn exception_irq_handler() {
    error!("IRQ Exceptions!");
    loop { }
}

#[cfg(any(target_arch="aarch64"))]
fn exception_fiq_handler() {
    error!("FIQ Exceptions!");
    loop { }
}

#[cfg(any(target_arch="aarch64"))]
fn exception_serror_handler() {
    error!("SErrorExceptions!");
    loop { }
}