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


use interrupts::{AvailableSegmentSelector, get_segment_selector};



fn syscall_dispatcher(syscall_number: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64, arg6: u64) {
    trace!("syscall_dispatcher: num={} arg1={} arg2={} arg3={} arg4={} arg5={} arg6={}",
            syscall_number, arg1, arg2, arg3, arg4, arg5, arg6);
}


pub fn init(privilege_stack_top_usable: usize) {
    unsafe { enable_syscall_sysret(privilege_stack_top_usable); }
}


#[allow(private_no_mangle_fns)]
#[no_mangle]
#[naked]
unsafe extern "C" fn syscall_handler() {

    // here, rcx = userland IP, r11 = userland EFLAGS

    // switch to the kernel's privilege stack (TSS.RSP0)
    // link to tifflin: https://github.com/thepowersgang/rust_os/blob/deb156d263e0a0af9195955cccfc150ea12f466f/Kernel/Core/arch/amd64/start.asm#L335
    asm!("swapgs");
    // FIXME: TODO: use proper TLS to save user's rsp, temporarily using r14
    asm!("mov r13, rsp" : : : : "intel"); // copy userspace RSP to r13 for now
    
    // swap to the current kernel rsp. Right now I'm placing a pointer to the TSS.RSP0 directly into the hidden GSBASE, hence the offset of 0x0
    asm!("mov rsp, gs:[0x0]" : : : "memory" : "intel", "volatile");  

    // save the old stack frame (unsure if necessary)

    asm!("push r13" : : : : "intel"); // cuz we're temporarily using r13 to save userspace rsp
    // asm!("push rbp" : : : : "intel");
    asm!("push rcx; push r11" : : : : "intel"); // RCX = userland IP, R11 = userland EFLAGS
    // asm!("push r11" : : : : "intel"); // stack must be 16-byte aligned, so just pushing another random item so we push an even number of things


    ::drivers::serial_port::serial_out("IN SYSCALL HANDLER!\n");


    let (rax, rdi, rsi, rdx, r10, r9, r8): (u64, u64, u64, u64, u64, u64, u64); 
    asm!("" : "={rax}"(rax), "={rdi}"(rdi), "={rsi}"(rsi), "={rdx}"(rdx), "={r10}"(r10), "={r9}"(r9), "={r8}"(r8)  : : : "intel");
    
    // here: once the registers are remapped to local rust vars, then we can do anything we want
    
    let curr_id = ::task::get_current_task_id().into();

    // FIXME:  macros like trace!() below cause the stack contents to be clobbered, which ruins the popping of r11 below. 
    // Since R11's pushed val on the stack is ruined, when sysretq returns to ring 3, interrupts are no longer properly enabled. 
    // FIXME: TODO: fix this to either remove macros like this or to do something else here to avoid stack contents being ruined.

    trace!("syscall_handler: curr_tid={}  rax={:#x} rdi={:#x} rsi={:#x} rdx={:#x} r10={:#x} r9={:#x} r8={:#x}",
           curr_id, rax, rdi, rsi, rdx, r10, r9, r8);
    /*

    // asm!("sti");


    // because we use the same calling convention for syscalls that Rust uses for functions, 
    // the syscall arguments are already in the proper registers and order that Rust functions expect
    let result = syscall_dispatcher(rax, rdi, rsi, rdx, r10, r9, r8); 
    
    trace!("syscall_handler: interrupts enabled={}", ::interrupts::interrupts_enabled());
    

    // trace!("syscall_handler: entering infinite loop!");
    // loop { }
    // trace!("syscall_handler: SHOULDN'T BE HERE!...");



    // asm!("cli");

    */

    // asm!("pop r11" : : : : "intel"); // pop random thing off because of the 16-byte stack alignment (above)
    asm!("pop r11; pop rcx" : : : : "intel"); // recover userland registers
    //asm!("or r11, 0x200" : : : : "intel"); // TRYING this: force enable interrupts in userspace: causes a GPF
    // TODO: restore user's rsp properly using TLS data from gs
    // asm!("mov rsp, gs:[0x10]" : : : : "intel");
    // asm!("pop rbp" : : : : "intel");
    asm!("pop rsp" : : : : "intel"); // cuz we're temporarily using r13 to save userspace rsp

    // restore current GS back into GSBASE
    asm!("swapgs");
    asm!("sysretq");

}


unsafe fn enable_syscall_sysret(privilege_stack_top_usable: usize) {

    // set up GS segment using its MSR, it should point to a special kernel stack that we can use for this.
    // Right now we're just using the save privilege level stack used for interrupts from user space (TSS's rsp 0)
    // in the future, this will be a separate value per-thread, using thread-local storage
    use x86_64::registers::msr::{IA32_GS_BASE, IA32_KERNEL_GS_BASE, IA32_FMASK, IA32_STAR, IA32_LSTAR, wrmsr};
    use alloc::boxed::Box;
    let top_ptr = Box::new(privilege_stack_top_usable);
    let raw_ptr = Box::into_raw(top_ptr) as u64;
    wrmsr(IA32_KERNEL_GS_BASE, raw_ptr);
    wrmsr(IA32_GS_BASE, raw_ptr); 
    println_unsafe!("Set KERNEL_GS_BASE and GS_BASE to privilege_stack_top_usable={:#x}", privilege_stack_top_usable);
    
    // set a kernelspace entry point for the syscall instruction from userspace
    wrmsr(IA32_LSTAR, syscall_handler as u64);

	// set up user code segment and kernel code segment
    // not sure if user cs segment should be 0x1B or 0x18. Beelzebub (vercas) sets it as 0x18 because it should be an offset (?)
    let user_cs = get_segment_selector(AvailableSegmentSelector::UserCode32).0 - 3; 
    let kernel_cs = get_segment_selector(AvailableSegmentSelector::KernelCode).0;
    let star_val: u32 = ((user_cs as u32) << 16) | (kernel_cs as u32); // this is what's recommended
    wrmsr(IA32_STAR, (star_val as u64) << 32);   //  [63:48] User CS, [47:32] Kernel CS
    println_unsafe!("Set IA32_STAR to {:#x}", star_val);

    // set up flags upon kernelspace entry into syscall_handler
    let rflags_interrupt_bitmask = 0x200;
    wrmsr(IA32_FMASK, rflags_interrupt_bitmask);  // clear interrupts during syscalls (if the bit is set here, it will be cleared upon a syscall)
}

