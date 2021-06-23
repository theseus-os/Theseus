//! This file is derived from `cortex-m-rt` crate, with `Reset`
//! function modified.

use core::{ptr, mem};

use crate::main;

#[link_section = ".vector_table.reset_vector"]
#[no_mangle]
pub static __RESET_VECTOR: unsafe extern "C" fn() -> ! = Reset;

/// Returns a pointer to the start of the heap
///
/// The returned pointer is guaranteed to be 4-byte aligned.
#[inline]
pub fn heap_start() -> usize {
    extern "C" {
        static mut __sheap: usize;
    }

    let p: usize;
    unsafe {
        asm!(
            "ldr  {r}, ={sheap}",
            r = out(reg) p,
            sheap = sym __sheap
        );
    }
    p
}

#[link_section = ".Reset"]
#[allow(non_snake_case)]
#[export_name = "Reset"]
unsafe extern "C" fn Reset() -> ! {
    extern "C" {
        // These symbols come from `link.ld`
        static mut __sbss: u32;
        static mut __ebss: u32;
        static mut __sdata: u32;
        static mut __edata: u32;
        static __sidata: u32;
    }

    asm!(
        "ldr r9, ={sdata}",
        "ldr r0, ={sbss}",
        "ldr r1, ={ebss}",
        "bl  {memzero}",
        "ldr r0, ={sdata}",
        "ldr r1, ={edata}",
        "ldr r2, ={sidata}",
        "bl  {memcopy}",
        "bl  {main}",
        sbss = sym __sbss,
        ebss = sym __ebss,
        sdata = sym __sdata,
        edata = sym __edata,
        sidata = sym __sidata,
        memzero = sym memzero,
        memcopy = sym memcopy,
        main = sym main
    );

    loop {}
}

extern "C" fn memzero(start: u32, end: u32) {
    let mut s = start as *mut u8;
    let e = end as *mut u8;
    while s < e {
        unsafe {
            ptr::write_volatile(s, mem::zeroed()); 
            s = s.offset(1);
        }
    }
}

extern "C" fn memcopy(dst_start: u32, dst_end: u32, src_start: u32) {
    let mut dst_s = dst_start as *mut u8;
    let dst_e = dst_end as *mut u8;
    let mut src_s = src_start as *const u8;
    while dst_s < dst_e {
        unsafe {
            ptr::write_volatile(dst_s, ptr::read_volatile(src_s));
            dst_s = dst_s.offset(1);
            src_s = src_s.offset(1);
        }
    }
}

pub union Vector {
    handler: unsafe extern "C" fn(),
    reserved: usize,
}

#[link_section = ".vector_table.exceptions"]
#[no_mangle]
pub static __EXCEPTIONS: [Vector; 14] = [
    // Exception 2: Non Maskable Interrupt.
    Vector {
        handler: NonMaskableInt,
    },
    // Exception 3: Hard Fault Interrupt.
    Vector { handler: HardFaultTrampoline },
    // Exception 4: Memory Management Interrupt [not on Cortex-M0 variants].
    #[cfg(not(armv6m))]
    Vector {
        handler: MemoryManagement,
    },
    #[cfg(armv6m)]
    Vector { reserved: 0 },
    // Exception 5: Bus Fault Interrupt [not on Cortex-M0 variants].
    #[cfg(not(armv6m))]
    Vector { handler: BusFault },
    #[cfg(armv6m)]
    Vector { reserved: 0 },
    // Exception 6: Usage Fault Interrupt [not on Cortex-M0 variants].
    #[cfg(not(armv6m))]
    Vector {
        handler: UsageFault,
    },
    #[cfg(armv6m)]
    Vector { reserved: 0 },
    // Exception 7: Secure Fault Interrupt [only on Armv8-M].
    #[cfg(armv8m)]
    Vector {
        handler: SecureFault,
    },
    #[cfg(not(armv8m))]
    Vector { reserved: 0 },
    // 8-10: Reserved
    Vector { reserved: 0 },
    Vector { reserved: 0 },
    Vector { reserved: 0 },
    // Exception 11: SV Call Interrupt.
    Vector { handler: SVCall },
    // Exception 12: Debug Monitor Interrupt [not on Cortex-M0 variants].
    #[cfg(not(armv6m))]
    Vector {
        handler: DebugMonitor,
    },
    #[cfg(armv6m)]
    Vector { reserved: 0 },
    // 13: Reserved
    Vector { reserved: 0 },
    // Exception 14: Pend SV Interrupt [not on Cortex-M0 variants].
    Vector { handler: PendSV },
    // Exception 15: System Tick Interrupt.
    Vector { handler: SysTick },
];

extern "C" {
    fn NonMaskableInt();

    #[cfg(not(armv6m))]
    fn MemoryManagement();

    #[cfg(not(armv6m))]
    fn BusFault();

    #[cfg(not(armv6m))]
    fn UsageFault();

    #[cfg(armv8m)]
    fn SecureFault();

    fn SVCall();

    #[cfg(not(armv6m))]
    fn DebugMonitor();

    fn PendSV();

    fn SysTick();
}

#[link_section = ".vector_table.interrupts"]
#[no_mangle]
pub static __INTERRUPTS: [unsafe extern "C" fn(); 240] = [{
    extern "C" {
        fn DefaultHandler();
    }

    DefaultHandler
}; 240];

#[allow(non_snake_case)]
#[export_name = "HardFaultTrampoline"]
pub extern "C" fn HardFaultTrampoline() {
    unsafe {
        extern "C" {
            fn HardFault();
        }
        asm!(
            "mov r0, lr",
            "mov r1, #4",
            "tst r0, r1",
            "bne 0f",
            "mrs r0, MSP",
            "b {hardfault_handler}",
            "0:",
            "mrs r0, PSP",
            "b {hardfault_handler}",
            hardfault_handler = sym HardFault
        )
    }
}
