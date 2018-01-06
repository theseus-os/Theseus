use interrupts::{AvailableSegmentSelector, get_segment_selector};


pub struct ArchTaskState {
    registers: Registers,
}

impl ArchTaskState {

    pub fn new() -> ArchTaskState {
        ArchTaskState { 
            registers: Registers::new(),
        }
    }


    /// Set the stack address.
    pub fn set_stack(&mut self, address: usize) {
        self.registers.set_stack(address);
    }


    /// performs the actual context switch.
    /// right now, `next` doesn't need to be mutable.
    #[inline(never)]
    #[naked]
    pub unsafe fn switch_to(&mut self, next: &ArchTaskState) {

        // debug!("switch_to [0]");

        // The following registers must be saved on x86_64:  (http://cons.mit.edu/sp17/x86-64-architecture-guide.html)
        // rbx, r12, r13, r14, r15, rsp, rbp
        // We also save rflags and the pdrp (cr3), both of which need to be saved

        // NOTE: xv6 saves rbx, rsp, rbp, rsi, rdi 
        // ..... do we need to save rsi and rdi?  
        // No, I don't think so. 
        
        // NOTE: osdev wiki saves rax, rbx, rcx, rdx, rsi, rdi, rsp, rbp, rip, rflags, cr3
        // http://wiki.osdev.org/Kernel_Multitasking
        // ..... do we need to save rax, rbx, rcx, rdx, rsi, rdi, rip? 
        // No, I don't think so. 


        /*
         * NOTE: address spaces are changed in the general `context_switch()` function now, not here!
         * as such, there is no need to modify cr3 here (it was changed just before calling this function)
         */

        // self.save_registers();
        // next.restore_registers();

        // save & restore rflags
        asm!("pushfq ; pop $0" : "=r"(self.registers.rflags) : : "memory" : "intel", "volatile");
        asm!("push $0 ; popfq" : : "r"(next.registers.rflags) : "memory" : "intel", "volatile");

        // save & restore rbx
        asm!("mov $0, rbx" : "=r"(self.registers.rbx) : : "memory" : "intel", "volatile");
        asm!("mov rbx, $0" : : "r"(next.registers.rbx) : "memory" : "intel", "volatile");
        
        // save & restore r12 - r15
        asm!("mov $0, r12" : "=r"(self.registers.r12) : : "memory" : "intel", "volatile");
        asm!("mov r12, $0" : : "r"(next.registers.r12) : "memory" : "intel", "volatile");
        asm!("mov $0, r13" : "=r"(self.registers.r13) : : "memory" : "intel", "volatile");
        asm!("mov r13, $0" : : "r"(next.registers.r13) : "memory" : "intel", "volatile");
        asm!("mov $0, r14" : "=r"(self.registers.r14) : : "memory" : "intel", "volatile");
        asm!("mov r14, $0" : : "r"(next.registers.r14) : "memory" : "intel", "volatile");
        asm!("mov $0, r15" : "=r"(self.registers.r15) : : "memory" : "intel", "volatile");
        asm!("mov r15, $0" : : "r"(next.registers.r15) : "memory" : "intel", "volatile");

        if true {
            // TESTING extra regs
            // save & restore rax, rcx, rdx, rdi, rsi
            asm!("mov $0, rax" : "=r"(self.registers.rax) : : "memory" : "intel", "volatile");
            asm!("mov rax, $0" : : "r"(next.registers.rax) : "memory" : "intel", "volatile");

            asm!("mov $0, rcx" : "=r"(self.registers.rcx) : : "memory" : "intel", "volatile");
            asm!("mov rcx, $0" : : "r"(next.registers.rcx) : "memory" : "intel", "volatile");

            asm!("mov $0, rdx" : "=r"(self.registers.rdx) : : "memory" : "intel", "volatile");
            asm!("mov rdx, $0" : : "r"(next.registers.rdx) : "memory" : "intel", "volatile");

            asm!("mov $0, rdi" : "=r"(self.registers.rdi) : : "memory" : "intel", "volatile");
            asm!("mov rdi, $0" : : "r"(next.registers.rdi) : "memory" : "intel", "volatile");

            asm!("mov $0, rsi" : "=r"(self.registers.rsi) : : "memory" : "intel", "volatile");
            asm!("mov rsi, $0" : : "r"(next.registers.rsi) : "memory" : "intel", "volatile");

        }

        // save & restore the stack pointer
        asm!("mov $0, rsp" : "=r"(self.registers.rsp) : : "memory" : "intel", "volatile");
        asm!("mov rsp, $0" : : "r"(next.registers.rsp) : "memory" : "intel", "volatile");

        // save & restore the base pointer
        asm!("mov $0, rbp" : "=r"(self.registers.rbp) : : "memory" : "intel", "volatile");
        asm!("mov rbp, $0" : : "r"(next.registers.rbp) : "memory" : "intel", "volatile");


        // enable interrupts again
        asm!("sti" : : : "memory" : "volatile");
    }


    // /// saves current registers into this Task's arch state
    // #[inline(never)]
    // #[naked]
    // unsafe fn save_registers(&mut self) {
    //     // save rflags
    //     asm!("pushfq ; pop $0" : "=r"(self.registers.rflags) : : "memory" : "intel", "volatile");

    //     // save rbx
    //     asm!("mov $0, rbx" : "=r"(self.registers.rbx) : : "memory" : "intel", "volatile");
        
    //     // save r12 - r15
    //     asm!("mov $0, r12" : "=r"(self.registers.r12) : : "memory" : "intel", "volatile");
    //     asm!("mov $0, r13" : "=r"(self.registers.r13) : : "memory" : "intel", "volatile");
    //     asm!("mov $0, r14" : "=r"(self.registers.r14) : : "memory" : "intel", "volatile");
    //     asm!("mov $0, r15" : "=r"(self.registers.r15) : : "memory" : "intel", "volatile");

    //     // save the stack pointer
    //     asm!("mov $0, rsp" : "=r"(self.registers.rsp) : : "memory" : "intel", "volatile");

    //     // save the base pointer
    //     asm!("mov $0, rbp" : "=r"(self.registers.rbp) : : "memory" : "intel", "volatile");

    // }


    // /// restores registers from this Task's arch state
    // #[inline(never)]
    // #[naked]
    // unsafe fn restore_registers(&self) {
    //     // restore rflags
    //     asm!("push $0 ; popfq" : : "r"(self.registers.rflags) : "memory" : "intel", "volatile");

    //     // restore rbx
    //     asm!("mov rbx, $0" : : "r"(self.registers.rbx) : "memory" : "intel", "volatile");
        
    //     // restore r12 - r15
    //     asm!("mov r12, $0" : : "r"(self.registers.r12) : "memory" : "intel", "volatile");
    //     asm!("mov r13, $0" : : "r"(self.registers.r13) : "memory" : "intel", "volatile");
    //     asm!("mov r14, $0" : : "r"(self.registers.r14) : "memory" : "intel", "volatile");
    //     asm!("mov r15, $0" : : "r"(self.registers.r15) : "memory" : "intel", "volatile");

    //     // restore the stack pointer
    //     asm!("mov rsp, $0" : : "r"(self.registers.rsp) : "memory" : "intel", "volatile");

    //     // restore the base pointer
    //     asm!("mov rbp, $0" : : "r"(self.registers.rbp) : "memory" : "intel", "volatile");

    // }


    // pub unsafe fn jump_to_userspace_sysret(&self, stack_ptr: usize, function_ptr: usize) {
    //  }

    pub unsafe fn jump_to_userspace(&self, stack_ptr: usize, function_ptr: usize) {
        
        // // first, save the current task's registers
        // self.save_registers();
        // // no need to restore registers here from a next task, since we're using special args instead


        // Steps to jumping to userspace:
        // 1) push stack segment selector (ss), i.e., the user_data segment selector
        // 2) push the userspace stack pointer
        // 3) push rflags, the control flags we wish to use
        // 4) push the code segment selector (cs), i.e., the user_code segment selector
        // 5) push the instruction pointer (rip) for the start of userspace, e.g., the function pointer
        // 6) set all other segment registers (ds, es, fs, gs) to the user_data segment, same as (ss)
        // 7) issue iret to return to userspace

        // debug!("Jumping to userspace with stack_ptr: {:#x} and function_ptr: {:#x}",
        //                   stack_ptr, function_ptr);
        // debug!("stack: {:#x} {:#x} func: {:#x}", *(stack_ptr as *const usize), *((stack_ptr - 8) as *const usize), 
        //                 *(function_ptr as *const usize));



        let ss: u16 = get_segment_selector(AvailableSegmentSelector::UserData64).0;
        let cs: u16 = get_segment_selector(AvailableSegmentSelector::UserCode64).0;

        

        // Redox sets the IOPL and interrupt enable flag using the following:  (3 << 12 | 1 << 9)
        // let mut flags: usize = 0;
        // asm!("pushf; pop $0" : "=r" (flags) : : "memory" : "volatile");
        let rflags: usize = (3 << 12) | (1 << 9); // what Redox does. TODO FIXME: Redox no longer sets IOPL bit
        
        // let rflags: usize = flags | 0x0200; // interrupts must be enabled in the rflags for the new userspace task
        // let rflags: usize = flags & !0x200; // quick test: disable interrupts in userspace

        // debug!("jump_to_userspace: rflags = {:#x}, userspace interrupts: {}", rflags, rflags & 0x200 == 0x200);


        // for Step 6, save rax before using it below
        // let mut rax_saved: usize = 0;
        // asm!("mov $0, rax" : "=r"(rax_saved) : : "memory" : "intel", "volatile");
        // asm!("mov ax, $0" : : "r"(ss) : "memory" : "intel", "volatile");
        asm!("mov ds, $0" : : "r"(ss) : "memory" : "intel", "volatile");
        asm!("mov es, $0" : : "r"(ss) : "memory" : "intel", "volatile");
        //asm!("mov fs, $0" : : "r"(ss) : "memory" : "intel", "volatile");
        //asm!("mov gs, $0" : : "r"(ss) : "memory" : "intel", "volatile");
        // asm!("mov rax, $0" : : "r"(rax_saved) : "memory" : "intel", "volatile");


        asm!("push $0" : : "r"(ss as usize) : "memory" : "intel", "volatile");
        asm!("push $0" : : "r"(stack_ptr) : "memory" : "intel", "volatile");
        asm!("push $0" : : "r"(rflags) : "memory" : "intel", "volatile");
        asm!("push $0" : : "r"(cs as usize) : "memory" : "intel", "volatile");
        asm!("push $0" : : "r"(function_ptr) : "memory" : "intel", "volatile");
        
        // Redox pushes an argument here too.

        // final step, use interrupt return to jump into Ring 3 userspace
        asm!("iretq" : : : "memory" : "intel", "volatile");
    }
}








#[repr(C, packed)] // only really necessary if we're writing to it from an .asm/.S file, which we're currently not doing
pub struct Registers {
    // docs here: http://cons.mit.edu/sp17/x86-64-architecture-guide.html

    
    /// 64-bit register destination index (destination of data copy instructions), first argument to functions
    rdi: usize, 
    /// 64-bit register source index (source of data copy instructions), second argument to functions
    rsi: usize, 

    /// 64-bit register A (accumulator), temp register usually used for passing back the return value
    rax: usize, 
    /// 64-bit register B (base)
    rbx: usize, 
    /// 64-bit register C (counter), fourth argument to functions
    rcx: usize, 
    /// 64-bit register D (data), third argument to functions
    rdx: usize, 
    
    /// 64-bit stack pointer register
    rsp: usize, 
    /// 64-bit stack base pointer register
    rbp: usize,

    /// used as 5th argument to functions
    r8: usize, 
    /// used as 6th argument to functions (final arg)
    r9: usize, 
    /// temporary register
    r10: usize, 
    /// temporary register
    r11: usize, 

    // r12-r15 must be saved 
    r12: usize, 
    r13: usize, 
    r14: usize, 
    r15: usize, 

    /// 64-bit instruction pointer register
    rip: usize,
    /// 64-bit flags register
    rflags: usize,
    /// 64-bit control register 3 (contains page dir pointer)
    cr3: usize,
}

impl Registers {
    pub fn new() -> Registers  {
        Registers {
            rax: 0, 
            rbx: 0, 
            rcx: 0, 
            rdx: 0, 
            rsi: 0, 
            rdi: 0, 
            rsp: 0, 
            rbp: 0,

            rip: 0,
            rflags: 0,
            cr3: 0,

            r8: 0, 
            r9: 0, 
            r10: 0, 
            r11: 0, 
            r12: 0, 
            r13: 0, 
            r14: 0, 
            r15: 0, 
        }
    }

  
    /// Set the stack address.
    pub fn set_stack(&mut self, address: usize) {
        debug_assert!(self.rsp == 0, "stack pointer (rsp) was already set!");
        self.rsp = address;
    }

}




#[inline(always)]
pub fn pause() {
    unsafe { asm!("pause" : : : : "intel", "volatile"); }
}

// #[inline(always)]
// pub fn enable_interrupts() {
//     unsafe { x86_64::instructions::interrupts::enable(); }
// }


// #[inline(always)]
// pub fn disable_interrupts() {
//     unsafe { x86_64::instructions::interrupts::disable(); }
// }


// #[inline(always)]
// pub fn interrupts_enabled() -> bool {
//     unsafe { 
//         let flags: u64;
// 		asm!("pushf; pop $0" : "=r" (flags) : : "memory" : "volatile");
// 		(flags & 0x200) != 0
//      }
// }