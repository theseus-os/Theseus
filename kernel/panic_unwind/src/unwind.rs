//! Taken from the gimli/unwind-rs/src/glue.rs file
//! 


use core::fmt;
use alloc::{
    sync::Arc,
    boxed::Box,
};
use gimli::{
    UnwindSection, 
    UnwindTableRow, 
    EhFrame, 
    BaseAddresses, 
    UninitializedUnwindContext, 
    FrameDescriptionEntry,
    Pointer,
    EndianSlice,
    NativeEndian,
    CfaRule,
    RegisterRule,
    X86_64
};
use registers::Registers;
use fallible_iterator::FallibleIterator;
use mod_mgmt::{
    CrateNamespace,
    metadata::{SectionType, StrongCrateRef, StrongSectionRef},
};
use memory::VirtualAddress;
use lsda;


/// This is the context/state that is used during unwinding and passed around
/// to the callback functions in the various unwinding stages, such as in `_Unwind_Resume()`. 
/// 
/// Because those callbacks follow an extern "C" ABI, this structure is passed as a pointer 
/// rather than directly by value or by reference.
/// Thus, it must be manually freed when unwinding is finished (or if it fails in the middle)
/// in order to avoid leaking memory, e.g., not dropping reference counts. 
pub struct UnwindingContext {
    stack_frame_iter: StackFrameIter,
}

impl Drop for UnwindingContext {
    fn drop(&mut self) {
        warn!("DROPPING UnwindingContext!");
    }
}



/// Due to lifetime and locking issues, we cannot store a direct reference to an unwind table row. 
/// Instead, here we store references to the objects needed to calculate/obtain an unwind table row.
#[derive(Debug)]
struct UnwindRowReference {
    caller: u64,
    eh_frame_sec_ref: StrongSectionRef,
    base_addrs: BaseAddresses,
}
impl UnwindRowReference {
    fn with_unwind_info<O, F>(&self, mut f: F) -> Result<O, &'static str>
        where F: FnMut(&FrameDescriptionEntry<NativeEndianSliceReader, usize>, &UnwindTableRow<NativeEndianSliceReader>) -> Result<O, &'static str>
    {
        let sec = self.eh_frame_sec_ref.lock();
        let size_in_bytes = sec.size();
        let sec_pages = sec.mapped_pages.lock();
        let eh_frame_vaddr = sec.start_address().value();
        assert_eq!(eh_frame_vaddr, sec_pages.start_address().value() + sec.mapped_pages_offset, "eh_frame address mismatch");
        let eh_frame_slice: &[u8] = sec_pages.as_slice(sec.mapped_pages_offset, size_in_bytes)?;
        let eh_frame = EhFrame::new(eh_frame_slice, NativeEndian);
        let mut unwind_ctx = UninitializedUnwindContext::new();
        let fde = eh_frame.fde_for_address(&self.base_addrs, self.caller, EhFrame::cie_from_offset).map_err(|_e| {
            error!("gimli error: {:?}", _e);
            "gimli error while finding FDE for address"
        })?;
        let unwind_table_row = fde.unwind_info_for_address(&eh_frame, &self.base_addrs, &mut unwind_ctx, self.caller).map_err(|_e| {
            error!("gimli error: {:?}", _e);
            "gimli error while finding unwind info for address"
        })?;
        
        debug!("FDE: {:?} ", fde);
        let mut instructions = fde.instructions(&eh_frame, &self.base_addrs);
        while let Some(instr) = instructions.next().map_err(|_e| {
            error!("FDE instructions gimli error: {:?}", _e);
            "gimli error while iterating through eh_frame FDE instructions list"
        })? {
            debug!("    FDE instr: {:?}", instr);
        }

        f(&fde, &unwind_table_row)
    }
}


/// A single frame in the stack, which contains
/// unwinding-related information for a single function call's stack frame.
#[derive(Debug)]
pub struct StackFrame {
    personality: Option<u64>,
    lsda: Option<u64>,
    initial_address: u64,
    caller_address: u64,
}

impl StackFrame {
    pub fn personality(&self) -> Option<u64> {
        self.personality
    }

    pub fn lsda(&self) -> Option<u64> {
        self.lsda
    }

    pub fn caller_address(&self) -> u64 {
        self.caller_address
    }

    pub fn initial_address(&self) -> u64 {
        self.initial_address
    }
}

type NativeEndianSliceReader<'i> = EndianSlice<'i, NativeEndian>;


pub struct NamespaceUnwinder {
    namespace: Arc<CrateNamespace>, 
    starting_crate: Option<StrongCrateRef>,
}

impl NamespaceUnwinder {
    /// Creates a new unwinder that can iterate over call stack frames 
    /// for function sections in the given `namespace` and in the given `starting_crate`.
    pub fn new(namespace: Arc<CrateNamespace>, starting_crate: Option<StrongCrateRef>) -> NamespaceUnwinder {
        NamespaceUnwinder { namespace, starting_crate }
    }

    pub fn namespace(&self) -> &CrateNamespace {
        &self.namespace
    }

    pub fn starting_crate(&self) -> Option<&StrongCrateRef> {
        self.starting_crate.as_ref()
    }
}


/// An iterator over all of the stack frames on the current stack,
/// which works in reverse calling order from the current function
/// up the call stack to the very first function on the stack,
/// at which point it will return `None`. 
/// 
/// This is a lazy iterator: the previous frame in the call stack
/// is only calculated upon invocation of the `next()` method. 
/// 
/// This can be used with the `FallibleIterator` trait.
pub struct StackFrameIter {
    /// A reference to the underlying unwinding data 
    /// that is used to traverse the stack frames.
    unwinder: NamespaceUnwinder,
    /// The register values that 
    /// These register values will change on each invocation of `next()`
    /// as different stack frames are successively iterated over.
    registers: Registers,
    /// Unwinding state related to the previous frame in the call stack:
    /// a reference to its row/entry in the unwinding table,
    /// and the Canonical Frame Address (CFA value) that is used to determine the next frame.
    state: Option<(UnwindRowReference, u64)>,
}

impl Drop for StackFrameIter {
    fn drop(&mut self) {
        warn!("Dropping StackFrameIter!");
    }
}

impl fmt::Debug for StackFrameIter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // write!(f, "StackFrameIter {{\nRegisters: {:?},\nstate: {:#X?}\n}}", self.registers, self.state)
        write!(f, "StackFrameIter {{\nRegisters: {:?}\n}}", self.registers)
    }
}


impl StackFrameIter {
    fn new(unwinder: NamespaceUnwinder, registers: Registers) -> Self {
        StackFrameIter {
            unwinder,
            registers,
            state: None,
        }
    }

    /// Returns the array of register values as they existed during the stack frame
    /// that is currently being iterated over. 
    /// This is necessary in order to restore the proper register values 
    /// before jumping to the **landing pad** (a cleanup function or exception catcher/panic handler)
    /// such that the landing pad function will actually execute properly with the right context.
    pub fn registers(&self) -> &Registers {
        &self.registers
    }

    /// Returns a reference to the underlying unwinder's context/state.
    pub fn unwinder(&self) -> &NamespaceUnwinder {
        &self.unwinder
    }
}

impl FallibleIterator for StackFrameIter {
    type Item = StackFrame;
    type Error = &'static str;

    fn next(&mut self) -> Result<Option<Self::Item>, Self::Error> {
        let registers = &mut self.registers;

        if let Some((unwind_row_ref, cfa)) = self.state.take() {
            let mut newregs = registers.clone();
            newregs[X86_64::RA] = None;
            unwind_row_ref.with_unwind_info(|_fde, row| {
                for &(reg, ref rule) in row.registers() {
                    trace!("rule {:?} {:?}", reg, rule);
                    // The stack pointer (RSP) is given by the CFA calculated during the previous iteration,
                    // there should *not* be a register rule defining the value of the RSP directly.
                    assert!(reg != X86_64::RSP); 
                    newregs[reg] = match *rule {
                        RegisterRule::Undefined => unreachable!(), // registers[reg],
                        RegisterRule::SameValue => Some(registers[reg].unwrap()), // not sure why this exists
                        RegisterRule::Register(r) => registers[r],
                        RegisterRule::Offset(n) => Some(unsafe { *((cfa.wrapping_add(n as u64)) as *const u64) }),
                        RegisterRule::ValOffset(n) => Some(cfa.wrapping_add(n as u64)),
                        RegisterRule::Expression(_) => unimplemented!(),
                        RegisterRule::ValExpression(_) => unimplemented!(),
                        RegisterRule::Architectural => unreachable!(),
                    };
                }
                Ok(())
            })?;
            newregs[7] = Some(cfa);

            *registers = newregs;
            trace!("registers: {:?}", registers);
        }


        if let Some(return_address) = registers[X86_64::RA] {
            // we've reached the end of the stack, so we're done iterating
            if return_address == 0 {
                return Ok(None);
            }

            // The return address (RA register) points to the next instruction (1 byte past the call instruction),
            // since the processor has advanced it to the next instruction to continue executing after the function returns. 
            let caller = return_address - 1;
            debug!("caller is {:#X}", caller);

            // get the unwind info for the caller address
            let crate_ref = self.unwinder.namespace.get_crate_containing_address(
                VirtualAddress::new_canonical(caller as usize), 
                self.unwinder.starting_crate.as_ref(),
                false,
            ).ok_or("couldn't get crate containing caller address")?;
            let (eh_frame_sec_ref, base_addrs) = get_eh_frame_info(&crate_ref)
                .ok_or("couldn't get eh_frame section in caller's containing crate")?;

            let row_ref = UnwindRowReference { caller, eh_frame_sec_ref, base_addrs };
            let (cfa, frame) = row_ref.with_unwind_info(|fde, row| {
                trace!("ok: {:?} (0x{:x} - 0x{:x})", row.cfa(), row.start_address(), row.end_address());
                let cfa = match *row.cfa() {
                    CfaRule::RegisterAndOffset { register, offset } =>
                        registers[register].unwrap().wrapping_add(offset as u64),
                    _ => unimplemented!(),
                };
                trace!("cfa is 0x{:x}", cfa);
                let frame = StackFrame {
                    personality: fde.personality().map(|x| unsafe { deref_ptr(x) }),
                    lsda: fde.lsda().map(|x| unsafe { deref_ptr(x) }),
                    initial_address: fde.initial_address(),
                    caller_address: caller,
                };
                Ok((cfa, frame))
            })?;
            self.state = Some((row_ref, cfa));
            Ok(Some(frame))
        } else {
            Ok(None)
        }
    }
}

unsafe fn deref_ptr(ptr: Pointer) -> u64 {
    match ptr {
        Pointer::Direct(x) => x,
        Pointer::Indirect(x) => *(x as *const u64),
    }
}


pub trait FuncWithRegisters = Fn(Registers) -> Result<(), &'static str>;
type RefFuncWithRegisters<'a> = &'a dyn FuncWithRegisters;


/// This function saves the current CPU register values onto the stack (to preserve them)
/// and then invokes the given closure with those registers as the argument.
/// 
/// In general, this is useful for jumpstarting the unwinding procedure,
/// since we have to start from the current call frame and work backwards up the call stack 
/// while applying the rules for register value changes in each call frame
/// in order to arrive at the proper register values for a prior call frame.
pub fn invoke_with_current_registers<F>(f: F) -> Result<(), &'static str> 
    where F: FuncWithRegisters 
{
    let f: RefFuncWithRegisters = &f;
    trace!("in invoke_with_current_registers(): calling unwind_trampoline...");
    let result = unsafe { 
        let res_ptr = unwind_trampoline(&f);
        let res_boxed = Box::from_raw(res_ptr);
        *res_boxed
    };
    trace!("in invoke_with_current_registers(): returned from unwind_trampoline with retval: {:?}", result);
    return result;
    // this is the end of the code in this function, the following is just inner functions.

    /// This is an internal assembly function used by `invoke_with_current_registers()` 
    /// that saves the current register values by pushing them onto the stack
    /// before invoking the function "unwind_recorder" with those register values as the only argument.
    /// This is needed because the unwind info tables describe register values as operations (offsets/addends)
    /// that are relative to the current register values, so we must have those current values as a starting point.
    /// 
    /// The argument is a pointer to a function reference, so effectively a pointer to a pointer. 
    #[naked]
    #[inline(never)]
    unsafe fn unwind_trampoline(_func: *const RefFuncWithRegisters) -> *mut Result<(), &'static str> {
        // DO NOT touch RDI register, which has the `_func` function; it needs to be passed into unwind_recorder.
        asm!("
            # copy the stack pointer to RSI
            movq %rsp, %rsi
            pushq %rbp
            pushq %rbx
            pushq %r12
            pushq %r13
            pushq %r14
            pushq %r15
            # To invoke `unwind_recorder`, we need to put: 
            # (1) the func in RDI (it's already there, just don't overwrite it),
            # (2) the stack in RSI,
            # (3) a pointer to the saved registers in RDX.
            movq %rsp, %rdx   # pointer to saved regs (on the stack)
            call unwind_recorder
            # restore saved registers
            popq %r15
            popq %r14
            popq %r13
            popq %r12
            popq %rbx
            popq %rbp
            ret
        ");
        core::hint::unreachable_unchecked();
    }


    /// The calling convention dictates the following order of arguments: 
    /// * first arg in `RDI` register, the function (or closure) to invoke with the saved registers arg,
    /// * second arg in `RSI` register, the stack pointer,
    /// * third arg in `RDX` register, the saved register values used to recover execution context
    ///   after we change the register values during unwinding,
    #[no_mangle]
    unsafe extern "C" fn unwind_recorder(
        func: *const RefFuncWithRegisters,
        stack: u64,
        saved_regs: *mut SavedRegs,
    ) -> *mut Result<(), &'static str> {
        let func = &*func;
        let saved_regs = &*saved_regs;

        let mut registers = Registers::default();
        registers[X86_64::RBX] = Some(saved_regs.rbx);
        registers[X86_64::RBP] = Some(saved_regs.rbp);
        registers[X86_64::RSP] = Some(stack + 8); // the stack value passed in is one pointer width before the real RSP
        registers[X86_64::R12] = Some(saved_regs.r12);
        registers[X86_64::R13] = Some(saved_regs.r13);
        registers[X86_64::R14] = Some(saved_regs.r14);
        registers[X86_64::R15] = Some(saved_regs.r15);
        registers[X86_64::RA]  = Some(*(stack as *const u64));

        let res = func(registers);
        Box::into_raw(Box::new(res))
    }
}


#[derive(Debug)]
#[repr(C)]
struct LandingRegisters {
    rax: u64,
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rdi: u64,
    rsi: u64,
    rbp: u64,
    r8:  u64,
    r9:  u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rsp: u64,
    // rflags? cs,fs,gs?
}

#[derive(Debug)]
#[repr(C)]
pub struct SavedRegs {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
}



/// **Landing** refers to the process of jumping to a handler for a stack frame,
/// e.g., an unwinding cleanup function, or an exception "catch" block.
/// 
/// This function basically fills the actual CPU registers with the values in the given `LandingRegisters`
/// and then jumps to the exception handler (landing pad) pointed to by the stack pointer (RSP) in those `LandingRegisters`.
/// 
/// This is similar in design to how the latter half of a context switch routine
/// must restore the previously-saved registers for the next task.
pub unsafe fn land(regs: &Registers, landing_pad_address: u64) {
    let mut landing_regs = LandingRegisters {
        rax: regs[X86_64::RAX].unwrap_or(0),
        rbx: regs[X86_64::RBX].unwrap_or(0),
        rcx: regs[X86_64::RCX].unwrap_or(0),
        rdx: regs[X86_64::RDX].unwrap_or(0),
        rdi: regs[X86_64::RDI].unwrap_or(0),
        rsi: regs[X86_64::RSI].unwrap_or(0),
        rbp: regs[X86_64::RBP].unwrap_or(0),
        r8:  regs[X86_64::R8 ].unwrap_or(0),
        r9:  regs[X86_64::R9 ].unwrap_or(0),
        r10: regs[X86_64::R10].unwrap_or(0),
        r11: regs[X86_64::R11].unwrap_or(0),
        r12: regs[X86_64::R12].unwrap_or(0),
        r13: regs[X86_64::R13].unwrap_or(0),
        r14: regs[X86_64::R14].unwrap_or(0),
        r15: regs[X86_64::R15].unwrap_or(0),
        rsp: regs[X86_64::RSP].expect("in unwind::land(): RSP was None, \
            it must be set to the landing pad address (of the unwind cleanup function or exception handler)!"),
    };

    // Now place the landing pad function's address at the "bottom" of the stack
    // -- not really the bottom of the whole stack, just the last thing to be popped off after the landing_regs.
    landing_regs.rsp -= 8;
    // *(lr.rsp as *mut u64) = regs[X86_64::RA].expect("in unwind::land(): the return address was None");
    *(landing_regs.rsp as *mut u64) = landing_pad_address;
    trace!("unwind_lander regs: {:#X?}", landing_regs);
    unwind_lander(&landing_regs);


    #[naked]
    #[inline(never)]
    unsafe extern fn unwind_lander(_regs: *const LandingRegisters) {
        asm!("
            movq %rdi, %rsp
            popq %rax
            popq %rbx
            popq %rcx
            popq %rdx
            popq %rdi
            popq %rsi
            popq %rbp
            popq %r8
            popq %r9
            popq %r10
            popq %r11
            popq %r12
            popq %r13
            popq %r14
            popq %r15
            movq 0(%rsp), %rsp
            # now we jump to the actual landing pad function
            ret
        ");
        core::hint::unreachable_unchecked();
    }
}




/// Returns a tuple of .eh_frame section for the given `crate_ref`
/// and the base addresses (its .text section address and .eh_frame section address).
/// 
/// # Locking / Deadlock
/// Obtains the lock on the given `crate_ref` 
/// and the lock on all of its sections while iterating through them.
/// 
/// The latter lock on the crate's `rodata_pages` object will be held
/// for the entire lifetime of the returned object. 
fn get_eh_frame_info(crate_ref: &StrongCrateRef) -> Option<(StrongSectionRef, BaseAddresses)> {
    let parent_crate = crate_ref.lock_as_ref();

    let eh_frame_sec_ref = parent_crate.sections.values()
        .filter(|s| s.lock().typ == SectionType::EhFrame)
        .next()?;
    
    let eh_frame_vaddr = eh_frame_sec_ref.lock().start_address().value();
    let text_pages_vaddr = parent_crate.text_pages.as_ref()?.1.start.value();
    let base_addrs = BaseAddresses::default()
        .set_eh_frame(eh_frame_vaddr as u64)
        .set_text(text_pages_vaddr as u64);

    Some((eh_frame_sec_ref.clone(), base_addrs))
}


fn print_stack_frames(stack_frames: &mut StackFrameIter) {
    while let Some(frame) = stack_frames.next().expect("stack_frames.next() error") {
        info!("StackFrame: {:#X?}", frame);
        info!("  in func: {:?}", stack_frames.unwinder().namespace().get_section_containing_address(VirtualAddress::new_canonical(frame.initial_address() as usize), stack_frames.unwinder().starting_crate(), false));
        if let Some(lsda) = frame.lsda() {
            info!("  LSDA section: {:?}", stack_frames.unwinder().namespace().get_section_containing_address(VirtualAddress::new_canonical(lsda as usize), stack_frames.unwinder().starting_crate(), true));
        }
    }
}


/// Starts the unwinding procedure for the current task 
/// by working backwards up the call stack starting from the current stack frame.
pub fn start_unwinding() -> Result<(), &'static str> {
    // Here we have to be careful to have no resources waiting to be dropped/freed/released on the stack. 
    let unwinding_context_ptr = {
        let curr_task = task::get_my_current_task().ok_or("get_my_current_task() failed")?;
        let namespace = curr_task.get_namespace();
        let (mmi_ref, app_crate_ref, _is_idle_task) = { 
            let t = curr_task.lock();
            (t.mmi.clone(), t.app_crate.as_ref().map(|a| a.clone_shallow()), t.is_an_idle_task)
        };

        panic_wrapper::stack_trace(
            &mmi_ref.lock().page_table,
            &|instruction_pointer: VirtualAddress| {
                namespace.get_section_containing_address(instruction_pointer, app_crate_ref.as_ref(), false)
                    .map(|(sec_ref, offset)| (sec_ref.lock().name.clone(), offset))
            },
        );

        Box::into_raw(Box::new(
            UnwindingContext {
                stack_frame_iter: StackFrameIter::new(NamespaceUnwinder::new(namespace, app_crate_ref), Registers::default()),
            }
        ))
    };

    // IMPORTANT NOTE!!!!
    // From this point on, if there is a failure, we need to free the unwinding context pointer to avoid leaking things.


    // We pass a pointer to the unwinding context to this closure. 
    let res = invoke_with_current_registers(|registers| {
        // set the proper register values before we used the 
        {  
            // SAFE: we just created this pointer above
            let unwinding_context = unsafe { &mut *unwinding_context_ptr };
            unwinding_context.stack_frame_iter.registers = registers;

            // Skip the first three frames, which correspond to functions in the panic handlers themselves.
            unwinding_context.stack_frame_iter.next()
                .map_err(|_e| "error skipping call stack frame 0 in unwinder")?
                .ok_or("call stack frame 0 did not exist (we were trying to skip it)")?;
            unwinding_context.stack_frame_iter.next()
                .map_err(|_e| "error skipping call stack frame 1 in unwinder")?
                .ok_or("call stack frame 1 did not exist (we were trying to skip it)")?;
            unwinding_context.stack_frame_iter.next()
                .map_err(|_e| "error skipping call stack frame 2 in unwinder")?
                .ok_or("call stack frame 2 did not exist (we were trying to skip it)")?;
        }

        continue_unwinding(unwinding_context_ptr)
    });

    match &res {
        &Ok(()) => {
            debug!("unwinding procedure has reached the end of the stack.");
        }
        &Err(e) => {
            error!("BUG: unwinding the first stack frame returned unexpectedly. Error: {}", e);
        }
    }
    cleanup_unwinding_context(unwinding_context_ptr);

    res
}


/// Continues the unwinding process from the point it left off at, 
/// which is defined by the given unwinding context.
/// 
/// This returns an error upon failure, 
/// and an `Ok(())` when it reaches the end of the stack and there are no more frames to unwind.
/// When either value is returned (upon a return of any kind),
/// **the caller is responsible for cleaning up the given `UnwindingContext`.
/// 
/// Upon successfully continuing to iterate up the call stack, this function will actually not return at all. 
fn continue_unwinding(unwinding_context_ptr: *mut UnwindingContext) -> Result<(), &'static str> {
    let stack_frame_iter = unsafe { &mut (*unwinding_context_ptr).stack_frame_iter };
    
    trace!("continue_unwinding(): stack_frame_iter: {:#X?}", stack_frame_iter);
    
    let (mut regs, landing_pad_address) = if let Some(frame) = stack_frame_iter.next().map_err(|e| {
        error!("continue_unwinding: error getting next stack frame in the call stack: {}", e);
        "continue_unwinding: error getting next stack frame in the call stack"
    })? {
        info!("Unwinding StackFrame: {:#X?}", frame);
        info!("  In func: {:?}", stack_frame_iter.unwinder().namespace().get_section_containing_address(VirtualAddress::new_canonical(frame.initial_address() as usize), stack_frame_iter.unwinder().starting_crate(), false));
        info!("  Regs: {:?}", stack_frame_iter.registers());

        if let Some(lsda) = frame.lsda() {
            let lsda = VirtualAddress::new_canonical(lsda as usize);
            if let Some((lsda_sec_ref, _)) = stack_frame_iter.unwinder().namespace().get_section_containing_address(lsda, stack_frame_iter.unwinder().starting_crate(), true) {
                info!("  parsing LSDA section: {:?}", lsda_sec_ref);
                let sec = lsda_sec_ref.lock();
                let starting_offset = sec.mapped_pages_offset + (lsda.value() - sec.address_range.start.value());
                let length_til_end_of_mp = sec.address_range.end.value() - lsda.value();
                let sec_mp = sec.mapped_pages.lock();
                let lsda_slice = sec_mp.as_slice::<u8>(starting_offset, length_til_end_of_mp)
                    .map_err(|_e| "continue_unwinding(): couldn't get LSDA pointer as a slice")?;
                let table = lsda::GccExceptTable::new(lsda_slice, NativeEndian, frame.initial_address());

                // let mut iter = table.call_site_table_entries().unwrap();
                // while let Some(entry) = iter.next().unwrap() {
                //     debug!("{:#X?}", entry);
                // }

                let entry = table.call_site_table_entry_for_address(frame.caller_address()).map_err(|e| {
                    error!("continue_unwinding(): couldn't find a call site table entry for this stack frame's caller address. Error: {}", e);
                    "continue_unwinding(): couldn't find a call site table entry for this stack frame's caller address."
                })?;

                debug!("Found call site entry for address {:#X}: {:#X?}", frame.caller_address(), entry);
                (stack_frame_iter.registers().clone(), entry.landing_pad_address())
            } else {
                error!("  BUG: couldn't find LSDA section (.gcc_except_table) for LSDA address: {:#X}", lsda);
                return Err("BUG: couldn't find LSDA section (.gcc_except_table) for LSDA address specified in stack frame");
            }
        } else {
            trace!("continue_unwinding(): stack frame has no LSDA");
            return continue_unwinding(unwinding_context_ptr);
        }
    } else {
        trace!("continue_unwinding(): NO REMAINING STACK FRAMES");
        return Ok(());
    };

    // Jump to the actual landing pad function, or rather, a function that will jump there after setting up register values properly.
    debug!("*** JUMPING TO LANDING PAD FUNCTION AT {:#X}", landing_pad_address);
    // Once the unwinding cleanup function is done, it will call _Unwind_Resume (technically, it jumps to it),
    // and pass the value in the landing registers' RAX register as the argument to _Unwind_Resume. 
    // So, whatever we put into RAX in the landing regs will be placed into the first arg (RDI) in _Unwind_Resume.
    // This is arch-specific; for x86_64 the transfer is from RAX -> RDI, for ARM/AARCH64, the transfer is from R0 -> R1 or X0 -> X1.
    // See this for more mappings: <https://github.com/rust-lang/rust/blob/master/src/libpanic_unwind/gcc.rs#L102>
    regs[gimli::X86_64::RAX] = Some(unwinding_context_ptr as u64);
    debug!("    set RAX value to {:#X?}", regs[gimli::X86_64::RAX]);
    unsafe {
        land(&regs, landing_pad_address);
    }
    error!("BUG: call to unwind::land() returned, which should never happen!");
    Err("BUG: call to unwind::land() returned, which should never happen!")
}


/// This function is automatically jumped to after each unwinding cleanup routine finishes executing,
/// so it's basically the return address of every cleanup routine.
/// Thus, this is a middle point in the unwinding execution flow; 
/// here we need to continue (*resume*) the unwinding procedure 
/// by basically figuring out where we just came from and picking up where we left off. 
/// That logic is performed in `unwind_tracer()`, see that function for more.
#[no_mangle]
pub unsafe extern "C" fn _Unwind_Resume(unwinding_context_ptr: *mut UnwindingContext) -> ! {
    trace!("_Unwind_Resume: unwinding_context_ptr value: {:#X}", unwinding_context_ptr as usize);

    match continue_unwinding(unwinding_context_ptr) {
        Ok(()) => {
            debug!("_Unwind_Resume: continue_unwinding() returned Ok(), meaning it's at the end of the call stack.");
        }
        Err(e) => {
            error!("BUG: in _Unwind_Resume: continue_unwinding() returned an error: {}", e);
        }
    }
    // here, cleanup the unwinding state and kill the task
    cleanup_unwinding_context(unwinding_context_ptr);

    warn!("Looping at the end of _Unwind_Resume()!");
    loop { }
}


/// This just drops the given `UnwindingContext` object pointed to by then given pointer.
/// 
/// TODO: we should also probably kill the task here, since there's nothing more we can really do.
fn cleanup_unwinding_context(unwinding_context_ptr: *mut UnwindingContext) {
    unsafe {
        let _ = Box::from_raw(unwinding_context_ptr);
    }
}
