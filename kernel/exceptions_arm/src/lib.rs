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
    let vector:u32;
    unsafe {  asm!("mrs $0, VBAR_EL1" : "=r"(vector) : : : "volatile") };
    //let exception = vector as *mut u32;
    let mut exceptions = VECTORS.lock();

    let exception = exceptions.as_ptr() as u32;
//    let exception = exception + 0x800 - (exception % 0x800);
    debug!("Wenqiu : {:X}", exception);
    
    let mut addr = exception;
    for i in 0..16 {
        unsafe { 
            let handler = exception_default_handler as u32;
            let handler = handler as u32;
            unsafe { debug!("Wenqiu : {:X}: {:X}", addr, handler); }
            let test = &exceptions[i] as *const _ as u32;
            unsafe { debug!("Wenqiu entry : {:X}: {:X}", test, handler); }
            //let ins = 0x98000000 as u32 - (test as u32 - handler as u32)/4;
            //let exception = (vector + 4) as *mut u32;
            //*exception = 0x9400e5fc;
            let ins = 0x98000000 as u32 - (test as u32 - handler as u32)/4;
            unsafe { debug!("Wenqiu entry : {:X}: {:X}", test, ins); }
            let ins = assemble_inst_bl(test, handler);
            unsafe { debug!("Wenqiu entry : {:X}: {:X}", test, ins); }
            exceptions[i].instructions[0] = ins;
        }
        
    }

    unsafe {
        asm!("
            msr VBAR_EL1, x0;
            dsb ish; 
            isb; " : :"{x0}"(exception): : "volatile");
    }

    debug!("WEnqiu: {}", exception);

    unsafe { *(0x400 as *mut u32) = 'd' as u32; }



    //return Frame::containing_address(PhysicalAddress::new_canonical(p4))
}

fn assemble_inst_bl(vector:u32, handler: u32) -> u32 {
    const INST_BL:u32 = 0x94000000;
    const BL_RANGE:u32 = 0x4000000;
    let dest_addr = handler as u32;
    let src_addr = vector;// as *const _ as u32;
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

#[cfg(any(target_arch="aarch64"))]
fn exception_default_handler() {
    debug!("Exceptions!");
    loop { }
}