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


pub fn init(privilege_stack_top_usable: usize) {
    unsafe { enable_syscall_sysret(privilege_stack_top_usable); }
}


#[no_mangle]
pub unsafe extern "C" fn syscall_handler() {
    
    asm!("movq r12, 0x1234567890ABCDEF" : : : : "intel");

    // switch to the kernel's privilege stack (TSS.RSP0)
    // link to tifflin: https://github.com/thepowersgang/rust_os/blob/deb156d263e0a0af9195955cccfc150ea12f466f/Kernel/Core/arch/amd64/start.asm#L335
    asm!("swapgs");
    // FIXME: TODO: use proper TLS to save user's rsp
    
    // use the below line later when we're actually storing a TLS struct in KERNEL_GS_BASE
    // asm!("mov rsp, gs:[0x0]" : : : "memory" : "intel", "volatile"); // swap to the current kernel rsp. Right now I'm placing the TSS.RSP0 directly into the hidden GSBASE, hence the offset of 0x0
    asm!("mov rbx, gs:[0x0]" : : : "memory" : "intel", "volatile");
    asm!("sub rbx, 0x8" : : : : "intel");
    asm!("mov rsp, rbx" : : : : "intel");

    let x = 5;
    let val = x + 3 * x;
    // here we are using the privilege stack 0 as our stack

    debug!("IN SYSCALL HANDLER! {} ", val);
    


    
    loop { }

    // FIXME: TODO: restore user's rsp

    // restore current GS back into GSBASE
    asm!("swapgs");
    asm!("sysret");

}


const STAR:  u32 = 0xC000_0081;
const LSTAR: u32 = 0xC000_0082;
const FMASK: u32 = 0xC000_0084;

unsafe fn enable_syscall_sysret(privilege_stack_top_usable: usize) {

    // set up GS segment using its MSR, it should point to a special kernel stack that we can use for this.
    // Right now we're just using the save privilege level stack used for interrupts from user space (TSS's rsp 0)
    // in the future, this will be a separate value per-thread, using thread-local storage
    use x86_64::registers::msr::{IA32_KERNEL_GS_BASE, wrmsr};
    use alloc::boxed::Box;
    let top_ptr = Box::new(privilege_stack_top_usable);
    wrmsr(IA32_KERNEL_GS_BASE, Box::into_raw(top_ptr) as u64); 
    println_unsafe!("Set KERNEL_GS_BASE to privilege_stack_top_usable={:#x}", privilege_stack_top_usable);
    
    asm!("mov rax, $0" : : "r"(syscall_handler as u64) : "memory" : "intel");
	asm!("mov rdx, rax" : : : "memory" : "intel");
	asm!("shr rdx, 32" : : : "memory" : "intel");
	asm!("mov ecx, $0" : : "r"(LSTAR) : "memory" : "intel");
	asm!("wrmsr" : : : "memory" : "intel");

	// get user code segment and kernel code segment
    let user_cs = get_segment_selector(AvailableSegmentSelector::UserCode);
    let kernel_cs = get_segment_selector(AvailableSegmentSelector::KernelCode);
    let star_val: u32 = ((user_cs.0 as u32) << 16) | (kernel_cs.0 as u32);

	asm!("mov eax, 0x0" : : : "memory" : "intel");
	asm!("mov edx, $0" : : "r"(star_val) : "memory" : "intel");	//  [63:48] User CS, [47:32] Kernel CS
	asm!("mov ecx, $0" : : "r"(STAR) : "memory" : "intel");
	asm!("wrmsr" : : : "memory" : "intel");

	asm!("mov eax, 0x200" : : : "memory" : "intel"); // clear interrupts during syscalls (if the bit is set here, it will be cleared upon a syscall)
	asm!("mov edx, 0x0" : : : "memory" : "intel");
	asm!("mov ecx, $0" : : "r"(FMASK) : "memory" : "intel");
	asm!("wrmsr" : : : "memory" : "intel");
}

