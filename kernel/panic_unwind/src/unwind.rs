//! Taken from the gimli/unwind-rs/src/glue.rs file
//! 

use alloc::{
    sync::Arc,
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


/// Due to lifetime and locking issues, we cannot store a direct reference to an unwind table row. 
/// Instead, here we store references to the objects needed to calculate/obtain an unwind table row.
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
        let relevant_fde = eh_frame.fde_for_address(&self.base_addrs, self.caller, EhFrame::cie_from_offset).map_err(|_e| {
            error!("gimli error: {:?}", _e);
            "gimli error while finding FDE for address"
        })?;
        let mut unwind_table_row = relevant_fde.unwind_info_for_address(&eh_frame, &self.base_addrs, &mut unwind_ctx, self.caller).map_err(|_e| {
            error!("gimli error: {:?}", _e);
            "gimli error while finding unwind info for address"
        })?;
        f(&relevant_fde, &unwind_table_row)
    }
}


#[derive(Debug)]
pub struct StackFrame {
    personality: Option<u64>,
    lsda: Option<u64>,
    initial_address: u64,
}

impl StackFrame {
    pub fn personality(&self) -> Option<u64> {
        self.personality
    }

    pub fn lsda(&self) -> Option<u64> {
        self.lsda
    }

    pub fn initial_address(&self) -> u64 {
        self.initial_address
    }
}

pub trait Unwinder {
    fn trace<F>(&mut self, f: F) where F: FnMut(&mut StackFrames);
}

type NativeEndianSliceReader<'i> = EndianSlice<'i, NativeEndian>;


pub struct NamespaceUnwinder {
    namespace: Arc<CrateNamespace>, 
    starting_crate: Option<StrongCrateRef>,
}

impl NamespaceUnwinder {
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

impl Unwinder for NamespaceUnwinder {
    fn trace<F>(&mut self, mut f: F) where F: FnMut(&mut StackFrames) {
        trace!("in NamespaceUnwinder::trace()");
        registers(|registers| {
            let mut frames = StackFrames::new(self, registers);
            f(&mut frames)
        });
    }
}



pub struct StackFrames<'a> {
    unwinder: &'a mut NamespaceUnwinder,
    registers: Registers,
    state: Option<(UnwindRowReference, u64)>,
}

impl<'a> StackFrames<'a> {
    pub fn new(unwinder: &'a mut NamespaceUnwinder, registers: Registers) -> Self {
        StackFrames {
            unwinder,
            registers,
            state: None,
        }
    }

    pub fn registers(&mut self) -> &mut Registers {
        &mut self.registers
    }

    /// Returns a reference to the underlying unwinder's context/state.
    pub fn unwinder(&self) -> &NamespaceUnwinder {
        self.unwinder
    }
}

impl<'a> FallibleIterator for StackFrames<'a> {
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
                    assert!(reg != X86_64::RSP); // stack = cfa
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


        if let Some(mut caller) = registers[X86_64::RA] {
            // we've reached the end of the stack, so we're done iterating
            if caller == 0 {
                return Ok(None);
            }

            caller -= 1; // look at the previous instruction (since x86 advances the PC/IP to the next instruction automatically)
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


#[no_mangle]
#[inline(never)]
fn test_payload(r: Registers) {
    trace!("test_payload: registers {:#X?}", r);
}

#[no_mangle]
#[inline(never)]
pub fn test_invoke() {
    let mut tpf: UnwindPayload = &mut test_payload;
    unsafe { unwind_trampoline(&mut tpf) };
}


type UnwindPayload<'a> = &'a mut dyn FnMut(Registers);

pub fn registers<F>(mut f: F) where F: FnMut(Registers) {
    let mut f = &mut f as UnwindPayload;
    trace!("in registers(): calling unwind_trampoline...");
    unsafe { unwind_trampoline(&mut f) };
    trace!("in registers(): returned from unwind_trampoline.");

}


/// This function saves the current register values by pushing them onto the stack
/// before invoking the function "unwind_recorder" with those register values as the only argument.
/// 
/// The calling convention dictates that the first argument, the payload function, is passed in the RDI register.
#[naked]
#[inline(never)]
pub unsafe extern fn unwind_trampoline(_payload: *mut UnwindPayload) {
    // DO NOT touch RDI register, which has the `_payload` function; it needs to be passed into unwind_recorder.
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

#[naked]
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
     ret // HYPERSPACE JUMP :D
     ");
    core::hint::unreachable_unchecked();
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


/// The calling convention dictates the following order of arguments: 
/// * first arg in `RDI` register, the payload function,
/// * second arg in `RSI` register, the stack pointer,
/// * third arg in `RDX` register, the saved register values used to recover execution context
///   after we change the register values during unwinding.
#[no_mangle]
unsafe extern "C" fn unwind_recorder(payload: *mut UnwindPayload, stack: u64, saved_regs: *mut SavedRegs) {
    trace!("unwind_recorder: payload {:#X}, stack: {:#X}, saved_regs: {:#X}",
        payload as usize, stack, saved_regs as usize,
    );
    let payload = &mut *payload;
    trace!("unwind_recorder: deref'd payload");
    let saved_regs = &*saved_regs;
    trace!("unwind_recorder: deref'd saved_regs: {:#X?}", saved_regs);

    let mut registers = Registers::default();
    registers[X86_64::RBX] = Some(saved_regs.rbx);
    registers[X86_64::RBP] = Some(saved_regs.rbp);
    registers[X86_64::RSP] = Some(stack + 8); // the stack value passed in is one pointer width below the real RSP
    registers[X86_64::R12] = Some(saved_regs.r12);
    registers[X86_64::R13] = Some(saved_regs.r13);
    registers[X86_64::R14] = Some(saved_regs.r14);
    registers[X86_64::R15] = Some(saved_regs.r15);
    registers[X86_64::RA]  = Some(*(stack as *const u64));

    payload(registers);
}

pub unsafe fn land(regs: &Registers) {
    let mut lr = LandingRegisters {
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
        rsp: regs[X86_64::RSP].unwrap(),
    };
    lr.rsp -= 8;
    *(lr.rsp as *mut u64) = regs[X86_64::RA].unwrap();
    unwind_lander(&lr);
}