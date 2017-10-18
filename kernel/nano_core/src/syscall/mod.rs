//! Code for initializing and handling syscalls from userspace,
//! using the amd64 SYSCALL/SYSRET special functions.


//! To invoke from userspace: 
//! The syscall number is passed in the rax register. 
//! The parameters are in this order:  rdi, rsi, rdx, r10, r8, r9. 
//! The call is invoked with the "syscall" instruction. 
//! The syscall overwrites the rcx register. 
//! The return value is in rax.




// Registers
// MSRs
// These must be accessed through rdmsr and wrmsr
// STAR (0xC0000081) - Ring 0 and Ring 3 Segment bases, as well as SYSCALL EIP. 
// Low 32 bits = SYSCALL EIP, bits 32-47 are kernel segment base, bits 48-63 are user segment base.

// LSTAR (0xC0000082) - The kernel's RIP SYSCALL entry for 64 bit software.
// CSTAR (0xC0000083) - The kernel's RIP for SYSCALL in compatibility mode.
// SFMASK (0xC0000084) - The low 32 bits are the SYSCALL flag mask. If a bit in this is set, the corresponding bit in rFLAGS is cleared.
// Operation
// NOTE: these instructions assume a flat segmented memory model (paging allowed). They require that "the code-segment base, limit, and attributes (except for CPL) are consistent for all application and system processes." --AMD System programming

// SYSCALL loads CS from STAR 47:32. It masks EFLAGS with SFMASK. Next it stores EIP in ECX. It then loads EIP from STAR 32:0 and SS from STAR 47:32 + 8. It then executes.

// Note that the Kernel does not automatically have a kernel stack loaded. This is the handler's responsibility.

// SYSRET loads CS from STAR 63:48. It loads EIP from ECX and SS from STAR 63:48 + 8.

// Note that the User stack is not automatically loaded. Also note that ECX must be preserved.

// 64 bit mode
// The operation in 64 bit mode is the same, except that RIP is loaded from LSTAR, or CSTAR of in IA32-e submode (A.K.A. compatibility mode). It also respectively saves and loads RFLAGS to and from R11. As well, in Long Mode, userland CS will be loaded from STAR 63:48 + 16 on SYSRET. Therefore, you might need to setup your GDT accordingly.

// Moreover, SYSRET will return to compatibility mode if the operand size is set to 32 bits, which is, for instance, nasm's default. To explicitly request a return into long mode, set the operand size to 64 bits (e.g. "o64 sysret" with nasm).


use core::sync::atomic::{Ordering, compiler_fence};
use interrupts::{AvailableSegmentSelector, get_segment_selector};


// #[no_mangle]
fn syscall_dispatcher(syscall_number: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64, arg6: u64) -> u64{
    trace!("syscall_dispatcher: num={} arg1={} arg2={} arg3={} arg4={} arg5={} arg6={}",
            syscall_number, arg1, arg2, arg3, arg4, arg5, arg6);

    return 0x1234BEEF0123FEED;
}


pub fn init(privilege_stack_top_usable: usize) {
    enable_syscall_sysret(privilege_stack_top_usable);

    let result = syscall_dispatcher(0, 1, 2, 3, 4, 5, 6);
    trace!("fake result = {:#x}", result);
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





#[allow(private_no_mangle_fns)]
#[no_mangle]
#[naked]
unsafe extern "C" fn syscall_handler() {


    // switch to the kernel stack dedicated for syscall handling, and save the user task's details
    // link to similar features in tifflin: https://github.com/thepowersgang/rust_os/blob/deb156d263e0a0af9195955cccfc150ea12f466f/Kernel/Core/arch/amd64/start.asm#L335
    // here, rcx = user task's IP, r11 = user task's EFLAGS
    // The gs offsets used below must match the order of elements in the UserTaskGsData struct above!!!
    asm!("swapgs; \
          mov gs:[0x8],  rsp; \
          mov gs:[0x10], rcx; \
          mov gs:[0x18], r11; \
          mov rsp, gs:[0x0];"
          : : : "memory" : "intel", "volatile");
    // asm!("push r11" : : : : "intel"); // stack must be 16-byte aligned, so just pushing another random item so we push an even number of things
    let (rax, rdi, rsi, rdx, r10, r9, r8): (u64, u64, u64, u64, u64, u64, u64); 
    asm!("" : "={rax}"(rax), "={rdi}"(rdi), "={rsi}"(rsi), "={rdx}"(rdx), "={r10}"(r10), "={r9}"(r9), "={r8}"(r8)  : : "memory" : "intel", "volatile");
    compiler_fence(Ordering::SeqCst);

   
    // here: once the stack is set up and registers are saved and remapped to local rust vars, then we can do anything we want
    // asm!("sti"); // TODO: we could consider letting interrupts occur while in a system call. Probably should do that. 
    
    let curr_id = ::task::get_current_task_id();
    trace!("syscall_handler: curr_tid={}  rax={:#x} rdi={:#x} rsi={:#x} rdx={:#x} r10={:#x} r9={:#x} r8={:#x}",
           curr_id, rax, rdi, rsi, rdx, r10, r9, r8);


    // FYI, Rust's calling conventions is as follows:  RDI,  RSI,  RDX,  RCX,  R8,  R9,  R10,  others on stack
    let result: u64 = syscall_dispatcher(rax, rdi, rsi, rdx, r10, r9, r8); 



    // below here, we cannot do anything we want, we must restore userspace registers in an atomic fashion
    compiler_fence(Ordering::SeqCst);
    // asm!("cli");
    asm!("mov rax, $0" : : "r"(result) : : "intel", "volatile"); //  put result in rax for returning to userspace

    // we don't need to save the current kernel rsp back into the UserTaskGsData struct's kernel_stack member (gs:[0x0]), 
    // because we can just re-use the same rsp that was originally placed into TSS RSP0 (which is set on a context switch)
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
fn enable_syscall_sysret(privilege_stack_top_usable: usize) {

    // set up GS segment using its MSR, it should point to a special kernel stack that we can use for this.
    // Right now we're just using the save privilege level stack used for interrupts from user space (TSS's rsp 0)
    // in the future, this will be a separate value per-thread, using thread-local storage
    use x86_64::registers::msr::{IA32_GS_BASE, IA32_KERNEL_GS_BASE, IA32_FMASK, IA32_STAR, IA32_LSTAR, wrmsr};
    use alloc::boxed::Box;
    let gs_data: UserTaskGsData = UserTaskGsData {
        kernel_stack: privilege_stack_top_usable as u64,
        // the other 3 elements below are 0, but will be init'd at the entry of every syscall_handler invocation
        user_stack: 0,
        user_ip: 0,
        user_flags: 0, 
    };
    let gs_data_ptr = Box::into_raw(Box::new(gs_data)) as u64; // puts it on the kernel heap, and prevents it from being dropped
    unsafe { wrmsr(IA32_KERNEL_GS_BASE, gs_data_ptr); }
    unsafe { wrmsr(IA32_GS_BASE, gs_data_ptr); }
    debug!("Set KERNEL_GS_BASE and GS_BASE to include a kernel stack at {:#x}", privilege_stack_top_usable);
    
    // set a kernelspace entry point for the syscall instruction from userspace
    unsafe { wrmsr(IA32_LSTAR, syscall_handler as u64); }

	// set up user code segment and kernel code segment
    // I believe the cs segment below should be 0x18, not 0x1B, because it's an offset, not a true descriptor with privilege level masks. 
    //      Beelzebub (vercas) sets it as 0x18.
    let user_cs = get_segment_selector(AvailableSegmentSelector::UserCode32).0 - 3;   // FIXME: more correct to do "& (!0b11);" rather than "-3"
    let kernel_cs = get_segment_selector(AvailableSegmentSelector::KernelCode).0;   // FIXME: more correct to do "& (!0b11);" rather than "-3"
    let star_val: u32 = ((user_cs as u32) << 16) | (kernel_cs as u32); // this is what's recommended
    unsafe { wrmsr(IA32_STAR, (star_val as u64) << 32); }   //  [63:48] User CS, [47:32] Kernel CS
    debug!("Set IA32_STAR to {:#x}", star_val);

    // set up flags upon kernelspace entry into syscall_handler
    let rflags_interrupt_bitmask = 0x200;
    unsafe { wrmsr(IA32_FMASK, rflags_interrupt_bitmask); }  // clear interrupts during syscalls (if the bit is set here, it will be cleared upon a syscall)
}

