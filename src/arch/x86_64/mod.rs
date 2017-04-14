
struct ArchTaskState {
    registers: Registers,
}

impl ArchTaskState {

    pub fn new() -> ArchTaskState {
        ArchTaskState { 
            register: Registers::new(),
        }
    }


    /// performs the actual context switch.
    /// right now, `next` doesn't need to be mutable.
    #[inline(never)]
    #[naked]
    pub unsafe fn switch_to(&mut self, next: &ArchTaskState) {
        // The following registers must be saved on x86_64:  (http://cons.mit.edu/sp17/x86-64-architecture-guide.html)
        // rbx, r12, r13, r14, r15, rsp, rbp
        // We also save rflags and the pdrp (cr3), both of which need to be saved

        // NOTE: xv6 saves rbx, rsp, rbp, rsi, rdi 
        // ..... do we need to save rsi and rdi? 
        
        // NOTE: osdev wiki saves rax, rbx, rcx, rdx, rsi, rdi, rsp, rbp, rip, rflags, cr3
        // http://wiki.osdev.org/Kernel_Multitasking
        // ..... do we need to save rax, rbx, rcx, rdx, rsi, rdi, rip? 

        // swap the pdrp (page tables) iff they're different
        // threads within the same process will have the same cr3
        // for example, in UNIX-like OSes, all kernel threads have the same cr3 (single kernel address space)
        asm!("mov $0, cr3" : "=r"(self.registers.cr3) : : "memory" : "intel", "volatile");
        if next.registers.cr3 != self.registers.cr3 {
            asm!("mov cr3, $0" : : "r"(next.registers.cr3) : "memory" : "intel", "volatile");
        }

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

        // save & restore the stack pointer
        asm!("mov $0, rsp" : "=r"(self.registers.rsp) : : "memory" : "intel", "volatile");
        asm!("mov rsp, $0" : : "r"(next.registers.rsp) : "memory" : "intel", "volatile");

        // save & restore the base pointer
        asm!("mov $0, rbp" : "=r"(self.registers.rbp) : : "memory" : "intel", "volatile");
        asm!("mov rbp, $0" : : "r"(next.registers.rbp) : "memory" : "intel", "volatile");
    }
}








#[repr(C, packed)] // only really necessary if we're writing to it from an .asm/.S file, which we're currently not doing
struct Registers {
    // docs here: http://cons.mit.edu/sp17/x86-64-architecture-guide.html

    
    /// 64-bit register destination index (destination of data copy instructions), first argument to functions
    rdi: u64, 
    /// 64-bit register source index (source of data copy instructions), second argument to functions
    rsi: u64, 

    /// 64-bit register A (accumulator), temp register usually used for passing back the return value
    rax: u64, 
    /// 64-bit register B (base)
    rbx: u64, 
    /// 64-bit register C (counter), fourth argument to functions
    rcx: u64, 
    /// 64-bit register D (data), third argument to functions
    rdx: u64, 
    
    /// 64-bit stack pointer register
    rsp: u64, 
    /// 64-bit stack base pointer register
    rbp: u64,

    /// used as 5th argument to functions
    r8: u64, 
    /// used as 6th argument to functions (final arg)
    r9: u64, 
    /// temporary register
    r10: u64, 
    /// temporary register
    r11: u64, 

    // r12-r15 must be saved 
    r12: u64, 
    r13: u64, 
    r14: u64, 
    r15: u64, 

    /// 64-bit instruction pointer register
    rip: u64,
    /// 64-bit flags register
    rflags: u64,
    /// 64-bit control register 3 (contains page dir pointer)
    cr3: u64,
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

    pub fn create(page_table: u64, stack: u64) {
        let mut regs = Registers::new();
        regs.set_page_table(page_table);
        regs.set_stack(stack);
        regs
    }

    /// Set the page table address.
    pub fn set_page_table(&mut self, address: u64) {
        debug_assert!(self.cr3 == 0, "cr3 was already set!");
        self.cr3 = address;
    }

    /// Set the stack address.
    pub fn set_stack(&mut self, address: u64) {
        debug_assert!(self.rsp == 0, "stack pointer (rsp) was already set!");
        self.rsp = address;
    }

}




#[inline(always)]
pub fn pause() {
    unsafe { asm!("pause" : : : : "intel", "volatile"); }
}