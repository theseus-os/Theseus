use x86_64;
use interrupts::{AvailableSegmentSelector, get_segment_selector};

/// get the real, current value of cr3
pub fn get_page_table_register() -> usize {
    x86_64::registers::control_regs::cr3().0 as usize
}


pub struct ArchTaskState {
    registers: Registers,
}

impl ArchTaskState {

    pub fn new() -> ArchTaskState {
        ArchTaskState { 
            registers: Registers::new(),
        }
    }


    /// Set the page table address.
    pub fn set_page_table(&mut self, address: usize) {
        self.registers.set_page_table(address);
    }

    /// Get the page table address.
    pub fn get_page_table(&self) -> usize {
        self.registers.get_page_table()
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

        // swap the pdrp (page tables) iff they're different
        // threads within the same process will have the same cr3
        // for example, in UNIX-like OSes, all kernel threads have the same cr3 (single kernel address space).
        // currently our kernel shares one address space, so the cr3 should only change between user processes
        asm!("mov $0, cr3" : "=r"(self.registers.cr3) : : "memory" : "intel", "volatile");
        if next.registers.cr3 != self.registers.cr3 {
            warn!("cr3 was different! curr={:#x} next={:#x}", self.registers.cr3, next.registers.cr3);
            asm!("mov cr3, $0" : : "r"(next.registers.cr3) : "memory" : "intel", "volatile");
        }
        else {
            // debug!("cr3 was the same as expected.");
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


        // enable interrupts again
        asm!("sti" : : : "memory" : "volatile");
    }


    /// # Prerequisites for calling this function
    /// * self.rsp must be set to a new userspace stack before calling this. 
    pub unsafe fn jump_to_userspace(&self, stack_ptr: usize, function_ptr: usize) {
        // Steps to jumping to userspace:
        // 1) push stack segment selector (ss), i.e., the user_data segment selector
        // 2) push the userspace stack pointer
        // 3) push rflags, the control flags we wish to use
        // 4) push the code segment selector (cs), i.e., the user_code segment selector
        // 5) push the instruction pointer (rip) for the start of userspace, e.g., the function pointer
        // 6) set all other segment registers (ds, es, fs, gs) to the user_data segment, same as (ss)
        // 7) issue iret to return to userspace

        let ss: u16 = get_segment_selector(AvailableSegmentSelector::UserData).0;
        let cs: u16 = get_segment_selector(AvailableSegmentSelector::UserCode).0;

        // for now, disable interrupts from userspace
        // Redox sets ths IOPL and interrupt enable flag using the following:  (3 << 12 | 1 << 9)
        let mut flags: usize = 0;
        asm!("pushf; pop $0" : "=r" (flags) : : "memory" : "volatile");
        let rflags = flags & !0x0200;


        // for Step 6, save rax before using it below
        // let mut rax_saved: usize = 0;
        // asm!("mov $0, rax" : "=r"(rax_saved) : : "memory" : "intel", "volatile");
        // asm!("mov ax, $0" : : "r"(ss) : "memory" : "intel", "volatile");
        asm!("mov ds, $0" : : "r"(ss) : "memory" : "intel", "volatile");
        asm!("mov es, $0" : : "r"(ss) : "memory" : "intel", "volatile");
        asm!("mov fs, $0" : : "r"(ss) : "memory" : "intel", "volatile");
        asm!("mov gs, $0" : : "r"(ss) : "memory" : "intel", "volatile");
        // asm!("mov rax, $0" : : "r"(rax_saved) : "memory" : "intel", "volatile");


        asm!("push $0" : : "r"(ss as usize) : "memory" : "intel", "volatile");
        asm!("push $0" : : "r"(stack_ptr) : "memory" : "intel", "volatile");
        asm!("push $0" : : "r"(rflags) : "memory" : "intel", "volatile");
        asm!("push $0" : : "r"(cs as usize) : "memory" : "intel", "volatile");
        asm!("push $0" : : "r"(function_ptr) : "memory" : "intel", "volatile");
        

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

    pub fn create(page_table: usize, stack: usize) -> Registers {
        let mut regs = Registers::new();
        regs.set_page_table(page_table);
        regs.set_stack(stack);
        regs
    }

    /// Set the page table address.
    pub fn set_page_table(&mut self, address: usize) {
        debug_assert!(self.cr3 == 0, "cr3 was already set!");
        self.cr3 = address;
    }

    /// Get the page table address.
    pub fn get_page_table(&self) -> usize {
        self.cr3
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

#[inline(always)]
pub fn enable_interrupts() {
    unsafe { x86_64::instructions::interrupts::enable(); }
}


#[inline(always)]
pub fn disable_interrupts() {
    unsafe { x86_64::instructions::interrupts::disable(); }
}


#[inline(always)]
pub fn interrupts_enabled() -> bool {
    unsafe { 
        let flags: u64;
		asm!("pushf; pop $0" : "=r" (flags) : : "memory" : "volatile");
		(flags & 0x200) != 0
     }
}