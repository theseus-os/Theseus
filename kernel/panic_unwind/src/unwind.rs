//! Taken from the gimli/unwind-rs/src/glue.rs file
//! 

#![allow(nonstandard_style)]

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
    metadata::{StrongCrateRef, StrongSectionRef},
};
use memory::VirtualAddress;



pub type c_int = i32;
pub type c_void = u64; // doesn't really matter
pub type uintptr_t = usize;



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
        error!("\nWHOA DROPPING STACK FRAME ITER!");
    }
}

impl fmt::Debug for StackFrameIter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // write!(f, "StackFrameIter {{\nRegisters: {:?},\nstate: {:#X?}\n}}", self.registers, self.state)
        write!(f, "StackFrameIter {{\nRegisters: {:?}\n}}", self.registers)
    }
}


impl StackFrameIter {
    pub fn new(unwinder: NamespaceUnwinder, registers: Registers) -> Self {
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
            let (eh_frame_sec_ref, base_addrs) = super::get_eh_frame_info(&crate_ref)
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


type FuncWithUnwinderRegisters<'a> = &'a dyn Fn(NamespaceUnwinder, Registers);


/// This function saves the current CPU register values onto the stack (to preserve them)
/// and then invokes the given closure with two arguments: (1) the given unwinder, and (2) those registers.
/// 
/// In general, this is useful for jumpstarting the unwinding procedure,
/// since we have to start from the current call frame and work backwards up the call stack 
/// while applying the rules for register value changes in each call frame
/// in order to arrive at the proper register values for a prior call frame.
pub fn invoke_with_current_registers<F>(unwinder: NamespaceUnwinder, f: F) where F: Fn(NamespaceUnwinder, Registers) {
    let f: FuncWithUnwinderRegisters = &f;
    let uw_ptr: *mut NamespaceUnwinder = Box::into_raw(Box::new(unwinder));
    trace!("in invoke_with_current_registers(): calling unwind_trampoline...");
    unsafe { unwind_trampoline(&f, uw_ptr) };
    trace!("in invoke_with_current_registers(): returned from unwind_trampoline.");
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
    unsafe fn unwind_trampoline(_payload: *const FuncWithUnwinderRegisters, _unwinder: *mut NamespaceUnwinder, ) {
        // DO NOT touch RDI register, which has the `_payload` function; it needs to be passed into unwind_recorder.
        asm!("
            # move the _unwinder argument from arg 2 to arg 4
            movq %rsi, %rcx
            # copy the stack pointer to RSI
            movq %rsp, %rsi
            pushq %rbp
            pushq %rbx
            pushq %r12
            pushq %r13
            pushq %r14
            pushq %r15
            # To invoke `unwind_recorder`, we need to put: 
            # (1) the payload in RDI (it's already there, just don't overwrite it),
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
    /// * first arg in `RDI` register, the payload function,
    /// * second arg in `RSI` register, the stack pointer,
    /// * third arg in `RDX` register, the saved register values used to recover execution context
    ///   after we change the register values during unwinding,
    /// * fourth arg in `RCX` register, the argument containing the Unwinder.
    #[no_mangle]
    unsafe extern "C" fn unwind_recorder(
        payload: *const FuncWithUnwinderRegisters,
        stack: u64,
        saved_regs: *mut SavedRegs,
        unwinder_ptr: *mut NamespaceUnwinder
    ) {
        trace!("unwind_recorder: payload {:#X}, stack: {:#X}, saved_regs: {:#X}",
            payload as usize, stack, saved_regs as usize,
        );
        let payload = &*payload;
        trace!("unwind_recorder: deref'd payload");
        let saved_regs = &*saved_regs;
        trace!("unwind_recorder: deref'd saved_regs: {:#X?}", saved_regs);
        let unwinder = Box::from_raw(unwinder_ptr);

        let mut registers = Registers::default();
        registers[X86_64::RBX] = Some(saved_regs.rbx);
        registers[X86_64::RBP] = Some(saved_regs.rbp);
        registers[X86_64::RSP] = Some(stack + 8); // the stack value passed in is one pointer width below the real RSP
        registers[X86_64::R12] = Some(saved_regs.r12);
        registers[X86_64::R13] = Some(saved_regs.r13);
        registers[X86_64::R14] = Some(saved_regs.r14);
        registers[X86_64::R15] = Some(saved_regs.r15);
        registers[X86_64::RA]  = Some(*(stack as *const u64));

        payload(*unwinder, registers);
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





/// The type signature of the personality function, see `rust_eh_personality`.
type PersonalityFunction = extern "C" fn(
    version: c_int, 
    actions: _Unwind_Action,
    class: u64,
    object: *mut _Unwind_Exception,
    context: *mut _Unwind_Context
) -> _Unwind_Reason_Code;


#[cfg(not(test))]
#[lang = "eh_personality"]
#[no_mangle]
#[doc(hidden)]
/// This function will always be emitted as "rust_eh_personality" no matter what function name we give it here.
unsafe extern "C" fn rust_eh_personality(
    version: c_int, 
    actions: _Unwind_Action,
    class: u64,
    object: *mut _Unwind_Exception,
    context: *mut _Unwind_Context
) -> _Unwind_Reason_Code {
    error!("rust_eh_personality(): version: {:?}, actions: {:?}, class: {:?}, object: {:?}, context: {:?}",
        version, actions, class, object, context
    );

    if version != 1 {
        error!("rust_eh_personality(): version was {}, must be 1.", version);
        return _Unwind_Reason_Code::_URC_FATAL_PHASE1_ERROR;
    }

    _Unwind_Reason_Code::_URC_END_OF_STACK
}



#[repr(C)]
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum _Unwind_Action {
    _UA_SEARCH_PHASE = 1,
    _UA_CLEANUP_PHASE = 2,
    _UA_HANDLER_FRAME = 4,
    _UA_FORCE_UNWIND = 8,
    _UA_END_OF_STACK = 16,
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum _Unwind_Reason_Code {
    _URC_NO_REASON = 0,
    _URC_FOREIGN_EXCEPTION_CAUGHT = 1,
    _URC_FATAL_PHASE2_ERROR = 2,
    _URC_FATAL_PHASE1_ERROR = 3,
    _URC_NORMAL_STOP = 4,
    _URC_END_OF_STACK = 5,
    _URC_HANDLER_FOUND = 6,
    _URC_INSTALL_CONTEXT = 7,
    _URC_CONTINUE_UNWIND = 8,
    _URC_FAILURE = 9, // used only by ARM EHABI
}

pub type _Unwind_Exception_Class = u64;
pub type _Unwind_Exception_Cleanup_Fn = extern "C" fn(unwind_code: _Unwind_Reason_Code, exception: *mut _Unwind_Exception);


#[cfg(target_arch = "x86_64")]
pub const UNWINDER_PRIVATE_DATA_SIZE: usize = 6;
#[cfg(target_arch = "aarch64")]
pub const UNWINDER_PRIVATE_DATA_SIZE: usize = 2;


#[repr(C)]
pub struct _Unwind_Exception {
    pub exception_class: _Unwind_Exception_Class,
    pub exception_cleanup: _Unwind_Exception_Cleanup_Fn,
    pub private_contptr: Option<u64>,
    pub private: [_Unwind_Word; UNWINDER_PRIVATE_DATA_SIZE],
}

pub type _Unwind_Word = uintptr_t;
pub type _Unwind_Ptr = uintptr_t;

pub struct _Unwind_Context {
    pub lsda: u64,
    pub ip: u64,
    pub initial_address: u64,
    pub registers: *mut Registers,
}

pub type _Unwind_Trace_Fn = extern "C" fn(ctx: *mut _Unwind_Context, arg: *mut c_void) -> _Unwind_Reason_Code;

#[no_mangle]
pub unsafe extern "C" fn _Unwind_DeleteException(exception: *mut _Unwind_Exception) {
    ((*exception).exception_cleanup)(_Unwind_Reason_Code::_URC_FOREIGN_EXCEPTION_CAUGHT, exception);
    trace!("exception deleted.");
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetRegionStart(ctx: *mut _Unwind_Context) -> _Unwind_Ptr {
    (*ctx).initial_address as usize
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetTextRelBase(ctx: *mut _Unwind_Context) -> _Unwind_Ptr {
    unreachable!();
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetDataRelBase(ctx: *mut _Unwind_Context) -> _Unwind_Ptr {
    unreachable!();
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetLanguageSpecificData(ctx: *mut _Unwind_Context) -> *mut c_void {
    (*ctx).lsda as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_SetGR(ctx: *mut _Unwind_Context, reg_index: c_int, value: _Unwind_Word) {
    (*(*ctx).registers)[reg_index as u16] = Some(value as u64);
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_SetIP(ctx: *mut _Unwind_Context, value: _Unwind_Word) {
    (*(*ctx).registers)[X86_64::RA] = Some(value as u64);
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_GetIPInfo(ctx: *mut _Unwind_Context, ip_before_insn: *mut c_int) -> _Unwind_Word {
    *ip_before_insn = 0;
    (*ctx).ip as usize
}

#[no_mangle]
pub unsafe extern "C" fn _Unwind_FindEnclosingFunction(pc: *mut c_void) -> *mut c_void {
    pc // FIXME: implement this
}




/*
/// Unwind_RaiseException is the first entry point invoked by Rust's panic handler in the std lib. 
/// Obviously, the core lib doesn't do that, so we need to do it ourselves.
/// 
// FIXME: Set `unwind(allowed)` because we need to be able to unwind this function as
// part of its operation. But this means any panics in this function are undefined
// behaviour, and we don't currently ensure it doesn't panic.
#[unwind(allowed)]
#[no_mangle]
pub unsafe extern "C" fn _Unwind_RaiseException(exception: *mut _Unwind_Exception) -> _Unwind_Reason_Code {
    (*exception).private_contptr = None;
    invoke_with_current_registers(|registers| {
        if let Some(registers) = unwind_tracer(registers, exception) {
            land(&registers);
        }
    });
    unreachable!();
}



// /// This function is automatically jumped to after each unwinding cleanup routine finishes executing,
// /// so it's basically the return address of every cleanup routine.
// /// Thus, this is a middle point in the unwinding execution flow; 
// /// here we need to continue (*resume*) the unwinding procedure 
// /// by basically figuring out where we just came from and picking up where we left off. 
// /// That logic is performed in `unwind_tracer()`, see that function for more.
// #[no_mangle]
// pub unsafe extern "C" fn _Unwind_Resume(exception: *mut _Unwind_Exception) -> ! {
//     invoke_with_current_registers(|registers| {
//         if let Some(registers) = unwind_tracer(registers, exception) {
//             land(&registers);
//         }
//     });
//     unreachable!();
// }


unsafe fn unwind_tracer(registers: Registers, exception: *mut _Unwind_Exception) -> Option<Registers> {
    let mut unwinder = {
        let curr_task = task::get_my_current_task();
        let t = curr_task.lock();
        NamespaceUnwinder::new(t.namespace.clone(), t.app_crate.clone())
    };
    let mut frames = StackFrameIter::new(&mut unwinder, registers);

    // This first conditional is responsible for skipping all the frames 
    // that we have already unwound, and iterating to the frame that should be unwound next.
    // If we haven't unwound any frames yet, then this will be None.
    if let Some(contptr) = (*exception).private_contptr {
        loop {
            if let Some(frame) = frames.next().unwrap() {
                if frames.registers()[X86_64::RSP].unwrap() == contptr {
                    break;
                }
            } else {
                return None;
            }
        }
    }

    while let Some(frame) = frames.next().unwrap() {
        if let Some(personality_fn_vaddr) = frame.personality {
            trace!("HAS PERSONALITY");
            let personality_func: PersonalityFunction = core::mem::transmute(personality_fn_vaddr);

            let mut ctx = _Unwind_Context {
                lsda: frame.lsda.unwrap(),
                ip: frames.registers()[X86_64::RA].unwrap(),
                initial_address: frame.initial_address,
                registers: frames.registers(),
            };

            // Set this call frame (its stack pointer) as the currently-handled one
            // so that we know where to continue unwinding after the next invocation to _Unwind_Resume
            (*exception).private_contptr = frames.registers()[X86_64::RSP];

            // The gcc libunwind ABI specifies that phase 1 (searching for a landing pad) is optional,
            // so we just skip directly to running phase 2 (cleanup)
            match personality_func(1, _Unwind_Action::_UA_CLEANUP_PHASE, (*exception).exception_class, exception, &mut ctx) {
                _Unwind_Reason_Code::_URC_CONTINUE_UNWIND => (),
                _Unwind_Reason_Code::_URC_INSTALL_CONTEXT => return Some(frames.registers),
                x => panic!("wtf reason code {:?}", x),
            }
        }
    }
    None
}

*/

// #[no_mangle]
// pub unsafe extern "C" fn _Unwind_Backtrace(trace: _Unwind_Trace_Fn, trace_argument: *mut c_void) -> _Unwind_Reason_Code {
//     DwarfUnwinder::default().trace(|frames| {
//         while let Some(frame) = frames.next().unwrap() {
//             let mut ctx = _Unwind_Context {
//                 lsda: frame.lsda.unwrap_or(0),
//                 ip: frames.registers()[X86_64::RA].unwrap(),
//                 initial_address: frame.initial_address,
//                 registers: frames.registers(),
//             };

//             trace(&mut ctx, trace_argument);
//         }
//     });
//     _Unwind_Reason_Code::_URC_END_OF_STACK
// }
