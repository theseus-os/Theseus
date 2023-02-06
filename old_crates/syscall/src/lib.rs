//! Code for initializing and handling syscalls from userspace,
//! using the amd64 SYSCALL/SYSRET special functions.
//! 
//! To invoke from userspace: 
//! The syscall number is passed in the rax register. 
//! The parameters are in this order:  rdi, rsi, rdx, r10, r8, r9. 
//! The call is invoked with the "syscall" instruction. 
//! The syscall overwrites the rcx register. 
//! The return value is in rax.
#![no_std]
#![feature(asm)]
// #![feature(compiler_fence)]
#![feature(naked_functions)]

#[macro_use] extern crate log;
extern crate memory;
extern crate util;
extern crate gdt;
extern crate cpu;
extern crate alloc;
extern crate x86_64;
extern crate task;
extern crate dbus;

use core::sync::atomic::{Ordering, compiler_fence};
use util::c_str::{c_char, CStr, CString};
use gdt::{AvailableSegmentSelector, get_segment_selector};
use memory::VirtualAddress;



fn syscall_dispatcher(syscall_number: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64, arg6: u64) -> u64{
    trace!("syscall_dispatcher: num={} arg1={} arg2={} arg3={} arg4={} arg5={} arg6={}",
            syscall_number, arg1, arg2, arg3, arg4, arg5, arg6);
    let mut result = 0;

    match syscall_number{
        1 => {


            // we use CStr instead of CString to indicate a borrowed &str that we do not own
            // (userspace owns it)
            let src_cstr:  &CStr = unsafe { CStr::from_ptr(arg1 as *const c_char) }; 
            let dest_cstr: &CStr = unsafe { CStr::from_ptr(arg2 as *const c_char) };
            let msg_cstr:  &CStr = unsafe { CStr::from_ptr(arg3 as *const c_char) };
            trace!("Send message {} from {} to {}", msg_cstr, src_cstr, dest_cstr);

            // NOTE from Kevin: Wenqiu, do you need to create so many Strings? They are slow and require allocation.
            // For example, in syssend, do you need an owned String (CString), or does a &str work (CStr)?
            //let src  =  src_cstr.to_string_lossy().into_owned();
            //let dest = dest_cstr.to_string_lossy().into_owned();
            //let msg  =  msg_cstr.to_string_lossy().into_owned();

            use dbus::syssend;
            syssend(src_cstr, dest_cstr, msg_cstr); // Kevin note: don't use macros here, they serve no purpose
        },
        2 =>{

            let conn_name:  &CStr = unsafe { CStr::from_ptr(arg1 as *const c_char) }; 
            use dbus::sysrecv;

            let msg: &str = &sysrecv(conn_name);
            result = CString::new(msg).unwrap().as_ptr() as u64;
            //let mut i = 1;
            /*result = 0;
            for b in msg.as_bytes(){
                result = result + i*(b.clone() as u64);
                i = i* 0x100;
            }*/

            trace!("Receive message {}", msg);
        }, 
          
        _ => error!("Unknown/unhandled syscall number {}", syscall_number),
    }
                
    return result;    
}


pub fn init(syscall_stack_top_usable: VirtualAddress) {
    enable_syscall_sysret(syscall_stack_top_usable);
}



/// The structure that holds data related to syscall/sysret handling.
/// NOTE: DO NOT CHANGE THE ORDER OF THESE ELEMENTS, THE syscall_handler() REQUIRES THEM TO BE IN A CERTAIN ORDER.
#[repr(C)]
struct UserTaskGsData {
    // TODO: change this to a proper TLS data structure later, and then swap the current GS to it in the task switcher

    /// the kernel's rsp
    kernel_stack: u64, // offset 0x0 (0)
    /// the user task's rsp, which is found in rsp upon syscall entry and should be placed back into rsp upon sysret
    user_stack: u64, // offset 0x8 (8)
    /// the user task's instruction pointer, which is found in rcx upon syscall entry and should be placed back into rcx upon sysret
    user_ip: u64, // offset 0x10 (16)
    /// the user task's rflags, which is found in r11 upon syscall entry and should be placed back into r11 upon sysret
    user_flags: u64, // offset 0x18 (24)
}





#[no_mangle]
#[naked]
#[inline(never)]
unsafe extern "C" fn syscall_handler() {


    // switch to the kernel stack dedicated for syscall handling, and save the user task's details
    // here, rcx = user task's IP, r11 = user task's EFLAGS
    // The gs offsets used below must match the order of elements in the UserTaskGsData struct above!!!
    asm!("swapgs; \
          mov gs:[0x8],  rsp; \
          mov gs:[0x10], rcx; \
          mov gs:[0x18], r11; \
          mov rsp, gs:[0x0];"
          : : : "memory" : "intel", "volatile");

    // asm!("push r11" : : : : "intel"); // stack must be 16-byte aligned, so just pushing another random item so we push an even number of things
    let (rax, rdi, rsi, rdx, r10, r8, r9): (u64, u64, u64, u64, u64, u64, u64); 
    asm!("" : "={rax}"(rax), "={rdi}"(rdi), "={rsi}"(rsi), "={rdx}"(rdx), "={r10}"(r10), "={r8}"(r8), "={r9}"(r9)  : : "memory" : "intel", "volatile");
    compiler_fence(Ordering::SeqCst);

   
    // here: once the stack is set up and registers are saved and remapped to local rust vars, then we can do anything we want
    // asm!("sti"); // TODO: we could consider letting interrupts occur while in a system call. Probably should do that. 
    
    let curr_id = ::task::get_my_current_task_id();
    trace!("syscall_handler: (AP {}) task id={:?}  rax={:#x} rdi={:#x} rsi={:#x} rdx={:#x} r10={:#x} r8={:#x} r9={:#x}",
           cpu::current_cpu(), curr_id, rax, rdi, rsi, rdx, r10, r8, r9);


    // FYI, Rust's calling conventions is as follows:  RDI,  RSI,  RDX,  RCX,  R8,  R9,  R10,  others on stack
    // because we have 7 args here, the last one will be  placed onto the stack, so we cannot rely on the stack not being changed.
    let result: u64 = syscall_dispatcher(rax, rdi, rsi, rdx, r10, r8, r9); 



    // below here, we cannot do anything we want, we must restore userspace registers in an atomic fashion
    compiler_fence(Ordering::SeqCst);
    // asm!("cli");
    asm!("mov rax, $0" : : "r"(result) : : "intel", "volatile"); //  put result in rax for returning to userspace

    // we don't need to save the current kernel rsp back into the UserTaskGsData struct's kernel_stack member (gs:[0x0]), 
    // because we can just re-use the same rsp that was originally placed into TSS RSP0 (which is set on a task switch)
    asm!("
          mov rsp, gs:[0x8];  \
          mov rcx, gs:[0x10]; \
          mov r11, gs:[0x18];"
          : : : "memory" : "intel", "volatile");


    // restore current GS back into GSBASE
    asm!("swapgs");
    asm!("sysretq");

}


/// Configures and enables the usage and behavior of `syscall` and `sysret` instructions. 
fn enable_syscall_sysret(syscall_stack_pointer: VirtualAddress) {

    // set up GS segment using its MSR, it should point to a special kernel stack that we can use for this.
    // Right now we're just using the save privilege level stack used for interrupts from user space (TSS's rsp 0)
    // in the future, this will be a separate value per-thread, using thread-local storage
    use x86_64::registers::msr::{IA32_GS_BASE, IA32_KERNEL_GS_BASE, IA32_FMASK, IA32_STAR, IA32_LSTAR, wrmsr};
    use alloc::boxed::Box;
    let gs_data: UserTaskGsData = UserTaskGsData {
        kernel_stack: syscall_stack_pointer.value() as u64,
        // the other 3 elements below are 0, but will be init'd at the entry of every syscall_handler invocation
        user_stack: 0,
        user_ip: 0,
        user_flags: 0, 
    };
    let gs_data_ptr = Box::into_raw(Box::new(gs_data)) as u64; // puts it on the kernel heap, and prevents it from being dropped
    unsafe { wrmsr(IA32_KERNEL_GS_BASE, gs_data_ptr); }
    unsafe { wrmsr(IA32_GS_BASE, gs_data_ptr); }
    debug!("Set KERNEL_GS_BASE and GS_BASE to include a kernel stack at {:#x}", syscall_stack_pointer);
    
    // set a kernelspace entry point for the syscall instruction from userspace
    unsafe { wrmsr(IA32_LSTAR, syscall_handler as u64); }

	// set up user code segment and kernel code segment
    // I believe the cs segment below should be 0x18, not 0x1B, because it's an offset, not a true descriptor with privilege level masks. 
    //      Beelzebub (vercas) sets it as 0x18.
    let user_cs = get_segment_selector(AvailableSegmentSelector::UserCode32).0 & (!0b11); // TODO FIXME: should this be UserCode64 ??!?
    let kernel_cs = get_segment_selector(AvailableSegmentSelector::KernelCode).0;
    let star_val: u32 = ((user_cs as u32) << 16) | (kernel_cs as u32); // this is what's recommended
    unsafe { wrmsr(IA32_STAR, (star_val as u64) << 32); }   //  [63:48] User CS, [47:32] Kernel CS
    debug!("Set IA32_STAR to {:#x}", star_val);

    // set up flags upon kernelspace entry into syscall_handler
    let rflags_interrupt_bitmask = 0x200;
    unsafe { wrmsr(IA32_FMASK, rflags_interrupt_bitmask); }  // clear interrupts during syscalls (if the bit is set here, it will be cleared upon a syscall)
}

