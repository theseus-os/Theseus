//! Taken from the gimli/unwind-rs/src/glue.rs file
//! 

use alloc::vec::Vec;
use gimli::{UnwindSection, UnwindTable, UnwindTableRow, EhFrame, BaseAddresses, UninitializedUnwindContext, Pointer, Reader, EndianSlice, NativeEndian, CfaRule, RegisterRule, X86_64};
use registers::Registers;
use fallible_iterator::FallibleIterator;

pub struct StackFrames<'a, 'i> {
    unwinder: &'a mut DwarfUnwinder<'i>,
    registers: Registers,
    state: Option<(UnwindTableRow<NativeEndianSliceReader<'i>>, u64)>,
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

/// There exists one ObjectRecord per separate object file.
pub struct ObjectRecord<'i> {
    pub eh_frame: EhFrame<NativeEndianSliceReader<'i>>,
    pub bases: BaseAddresses,
}

pub struct DwarfUnwinder<'i> {
    pub cfi: Vec<ObjectRecord<'i>>,
    pub ctx: UninitializedUnwindContext<NativeEndianSliceReader<'i>>,
}

impl<'i> DwarfUnwinder<'i> {
    pub fn new(eh_frame_slice: &'i [u8], eh_frame_vaddr: u64, text_vaddr: u64) -> DwarfUnwinder {
        let eh_frame = EhFrame::new(eh_frame_slice, NativeEndian);
        let bases = BaseAddresses::default()
            .set_eh_frame(eh_frame_vaddr)
            .set_text(text_vaddr);

        let mut cfi = Vec::new();
        cfi.push(ObjectRecord { eh_frame, bases });

        DwarfUnwinder {
            cfi,
            ctx: UninitializedUnwindContext::new(),
        }
    }
}

impl<'i> Unwinder for DwarfUnwinder<'i> {
    fn trace<F>(&mut self, mut f: F) where F: FnMut(&mut StackFrames) {
        trace!("in Unwinder::trace()");
        registers(|registers| {
            let mut frames = StackFrames::new(self, registers);
            f(&mut frames)
        });
    }
}

struct UnwindInfo<R: Reader> {
    row: UnwindTableRow<R>,
    personality: Option<Pointer>,
    lsda: Option<Pointer>,
    initial_address: u64,
}

impl<'i> ObjectRecord<'i> {
    fn unwind_info_for_address(
        &self,
        ctx: &mut UninitializedUnwindContext<NativeEndianSliceReader<'i>>,
        address: u64,
    ) -> gimli::Result<UnwindInfo<NativeEndianSliceReader<'i>>> {
        let &ObjectRecord {
            ref eh_frame,
            ref bases,
            ..
        } = self;

        let fde = eh_frame.fde_for_address(bases, address, EhFrame::cie_from_offset)?;
        let row = fde.unwind_info_for_address(eh_frame, bases, ctx, address)?;

        Ok(UnwindInfo {
            row,
            personality: fde.personality(),
            lsda: fde.lsda(),
            initial_address: fde.initial_address(),
        })
    }
}

unsafe fn deref_ptr(ptr: Pointer) -> u64 {
    match ptr {
        Pointer::Direct(x) => x,
        Pointer::Indirect(x) => *(x as *const u64),
    }
}


impl<'a, 'i> StackFrames<'a, 'i> {
    pub fn new(unwinder: &'a mut DwarfUnwinder<'i>, registers: Registers) -> Self {
        StackFrames {
            unwinder,
            registers,
            state: None,
        }
    }

    pub fn registers(&mut self) -> &mut Registers {
        &mut self.registers
    }
}

impl<'a, 'i> FallibleIterator for StackFrames<'a, 'i> {
    type Item = StackFrame;
    type Error = gimli::Error;

    fn next(&mut self) -> Result<Option<StackFrame>, Self::Error> {
        let registers = &mut self.registers;

        if let Some((row, cfa)) = self.state.take() {
            let mut newregs = registers.clone();
            newregs[X86_64::RA] = None;
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
            newregs[7] = Some(cfa);

            *registers = newregs;
            trace!("registers:{:?}", registers);
        }


        if let Some(mut caller) = registers[X86_64::RA] {
            caller -= 1; // THIS IS NECESSARY
            debug!("caller is {:#X}", caller);

            // TODO FIXME: here, check to make sure this record contains the 
            let record = self.unwinder.cfi.iter().next().ok_or(gimli::Error::NoUnwindInfoForAddress)?;

            let UnwindInfo { row, personality, lsda, initial_address } = record.unwind_info_for_address(&mut self.unwinder.ctx, caller)?;

            trace!("ok: {:?} (0x{:x} - 0x{:x})", row.cfa(), row.start_address(), row.end_address());
            let cfa = match *row.cfa() {
                CfaRule::RegisterAndOffset { register, offset } =>
                    registers[register].unwrap().wrapping_add(offset as u64),
                _ => unimplemented!(),
            };
            trace!("cfa is 0x{:x}", cfa);

            self.state = Some((row, cfa));

            Ok(Some(StackFrame {
                personality: personality.map(|x| unsafe { deref_ptr(x) }),
                lsda: lsda.map(|x| unsafe { deref_ptr(x) }),
                initial_address,
            }))
        } else {
            Ok(None)
        }
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
}


/// This function saves the current register values by pushing them onto the stack
/// before 
/// 
/// The calling convention here is that the first argument will be placed into the `RDI` register,
/// i.e., the payload function.
#[naked]
#[inline(never)]
pub unsafe extern fn unwind_trampoline(_payload: *mut UnwindPayload) {
    asm!("
        # copy the stack pointer to RSI
        movq %rsp, %rsi
        # .cfi_def_cfa rsi, 8
        pushq %rbp
        # .cfi_offset rbp, -16
        pushq %rbx
        pushq %r12
        pushq %r13
        pushq %r14
        pushq %r15
        # To invoke `unwind_recorder`, we need to put: 
        # the payload in RDI (it's already there, just don't overwrite it),
        # the stack in RSI,
        # and a pointer to the saved registers in RDX.
        movq %rsp, %rdx   # pointer to saved regs (on the stack)
        # subq 0x08, %rsp   # allocate space for the return address (?) I REMOVED THIS, UNSURE IF NEEDED
        # .cfi_def_cfa rsp, 0x40
        call unwind_recorder
        addq 0x38, %rsp
        # .cfi_def_cfa rsp, 8
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
    registers[X86_64::RSP] = Some(stack + 8); // the stack value passed in is one 
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