use core::arch::global_asm;
use core::cell::UnsafeCell;
use core::fmt;

use cortex_a::registers::*;

use tock_registers::interfaces::Writeable;
use tock_registers::interfaces::Readable;
use tock_registers::registers::InMemoryRegister;

use gic::{qemu_virt_addrs, ArmGic, IntNumber, TargetCpu};
use irq_safety::{RwLockIrqSafe, MutexIrqSafe};
use memory::{PageTable};
use log::{info, error};
use spin::Once;

// Raw/Early exception handling is defined
// in this assembly file.
global_asm!(include_str!("table.s"));

// The global interrupt controller singleton
static GIC: MutexIrqSafe<Once<ArmGic>> = MutexIrqSafe::new(Once::new());

// Default Timer IRQ number on AArch64 as
// defined by Arm Manuals
pub const AARCH64_TIMER_IRQ: IntNumber = 30;

// Singleton which acts like an x86-style
// Interrupt Descriptor Table: it's an
// array of function pointers which are
// meant to handle IRQs. Synchronous
// Exceptions (syscalls) are not IRQs on
// aarch64; this crate doesn't expose any
// way to handle them at the moment.
static IRQ_HANDLERS: RwLockIrqSafe<[HandlerFunc; 256]> = RwLockIrqSafe::new([default_irq_handler; 256]);

/// Wrapper structs for memory copies of registers.
#[repr(transparent)]
struct SpsrEL1(InMemoryRegister<u64, SPSR_EL1::Register>);
struct EsrEL1(InMemoryRegister<u64, ESR_EL1::Register>);

/// The exception context as it is stored on the stack on exception entry.
#[repr(C)]
pub struct ExceptionContext {
    /// General Purpose Registers.
    gpr: [u64; 30],

    /// The link register, aka x30.
    lr: u64,

    /// Exception link register. The program counter at the time the exception happened.
    elr_el1: u64,

    /// Saved program status.
    spsr_el1: SpsrEL1,

    /// Exception syndrome register.
    esr_el1: EsrEL1,
}

type HandlerFunc = extern "C" fn(&ExceptionContext) -> bool;

// called for all exceptions other than interrupts
fn default_exception_handler(exc: &ExceptionContext, origin: &'static str) {
    log::error!("Kernel Panic: Unhandled Exception ({})\r\n{}", origin, exc);
    loop {}
}

// called for all unhandled interrupt requests
extern "C" fn default_irq_handler(exc: &ExceptionContext) -> bool {
    log::error!("Kernel Panic: Unhandled IRQ:\r\n{}", exc);
    loop {}
}

/// Please call (once) this before using this crate.
///
/// This initializes the Generic Interrupt Controller
/// using the addresses which are valid on qemu's "virt" VM.
pub fn init(page_table: &mut PageTable) -> Result<(), &'static str> {
    let mut gic = GIC.lock();

    let inner = ArmGic::map(page_table, qemu_virt_addrs::GICD, qemu_virt_addrs::GICC)?;
    let mut result = Err("The GIC has already been initialized!");
    gic.call_once(|| { result = Ok(()); inner });

    if result.is_ok() {
        let gic = gic.get_mut().unwrap();
        log::info!("Configuring the GIC");
        gic.set_gicc_state(true);
        gic.set_gicd_state(true);
        gic.set_minimum_int_priority(0);
    }

    result
}

pub fn enable_timer_interrupts() -> Result<(), &'static str> {
        extern "Rust" {
            // in assembly file
            static __exception_vector_start: UnsafeCell<()>;
        }

        // Set the exception handling vector, which
        // is an array of grouped aarch64 instructions.
        // see table.s for more info.
        unsafe { VBAR_EL1.set(__exception_vector_start.get() as u64) };

        // called everytime the timer ticks.
        extern "C" fn timer_handler(_exc: &ExceptionContext) -> bool {
            info!("timer int!");
            loop {}

            // return false if you haven't sent an EOI
            // so that the caller does it for you
        }

        // register the handler for the timer IRQ.
        register_interrupt(AARCH64_TIMER_IRQ, timer_handler)
            .map_err(|_| "An interrupt handler has already been setup for the timer IRQ number")?;

        // Route the IRQ to this core (implicit as IRQ < 32)
        // & Enable the interrupt.
        {
            let mut gic = GIC.lock();
            let gic = gic.get_mut().ok_or("GIC is uninitialized")?;

            // this has no effect (IRQ# < 32), just including it
            // to show how the function should be used
            gic.set_int_target(AARCH64_TIMER_IRQ, TargetCpu::ALL_CPUS);

            // enable routing of this interrupt
            gic.set_int_state(AARCH64_TIMER_IRQ, true);
        }

        // read the frequency (useless atm)
        let counter_freq_hz = CNTFRQ_EL0.get();
        log::info!("frq: {:?}", counter_freq_hz);

        // unmask the interrupt
        // enable the timer
        CNTP_CTL_EL0.write(
              CNTP_CTL_EL0::IMASK.val(0)
            + CNTP_CTL_EL0::ENABLE.val(1)
        );

        /* DEBUGGING CODE

        log::info!("timer: {:?}", CNTPCT_EL0.get());
        log::info!("ENABLE: {:?}",  CNTP_CTL_EL0.read(CNTP_CTL_EL0::ENABLE));
        log::info!("IMASK: {:?}",   CNTP_CTL_EL0.read(CNTP_CTL_EL0::IMASK));
        log::info!("ISTATUS: {:?}", CNTP_CTL_EL0.read(CNTP_CTL_EL0::ISTATUS));

        */

        log::info!("Unmasking all exceptions types");
        // unmask every kind of exception
        DAIF.write(
              DAIF::D::Unmasked
            + DAIF::A::Unmasked

            // regular IRQs
            + DAIF::I::Unmasked

            // fast IRQs (unimplemented atm)
            + DAIF::F::Unmasked,
        );

        Ok(())
}

/// Registers an interrupt handler at the given IRQ interrupt number.
///
/// The function fails if the interrupt number is reserved or is already in use.
///
/// # Arguments 
/// * `irq_num`: the interrupt (IRQ vector) that is being requested.
/// * `func`: the handler to be registered, which will be invoked when the interrupt occurs.
///
/// # Return
/// * `Ok(())` if successfully registered, or
/// * `Err(existing_handler_address)` if the given `irq_num` was already in use.
pub fn register_interrupt(irq_num: IntNumber, func: HandlerFunc) -> Result<(), *const HandlerFunc> {
    let mut handlers = IRQ_HANDLERS.write();
    let index = irq_num as usize;

    let value = handlers[index] as *const HandlerFunc;
    let default = default_irq_handler as *const HandlerFunc;

    if value == default {
        handlers[index] = func;
        Ok(())
    } else {
        error!("register_interrupt: the requested interrupt IRQ {} was already in use", irq_num);
        Err(value)
    }
}

/// Deregisters an interrupt handler, making it available to the rest of the system again.
///
/// As a sanity/safety check, the caller must provide the `interrupt_handler`
/// that is currently registered for the given IRQ `interrupt_num`.
/// This function returns an error if the currently-registered handler does not match 'func'.
///
/// # Arguments
/// * `interrupt_num`: the IRQ that needs to be deregistered
/// * `func`: the handler that should currently be stored for 'interrupt_num'
pub fn deregister_interrupt(irq_num: IntNumber, func: HandlerFunc) -> Result<(), *const HandlerFunc> {
    let mut handlers = IRQ_HANDLERS.write();
    let index = irq_num as usize;

    let value = handlers[index] as *const HandlerFunc;
    let func = func as *const HandlerFunc;

    if value == func {
        handlers[index] = default_irq_handler;
        Ok(())
    } else {
        error!("deregister_interrupt: Cannot free interrupt due to incorrect handler function");
        Err(value)
    }
}

/// Send an "end of interrupt" signal, notifying the interrupt chip that
/// the given interrupt request `irq` has been serviced.
pub fn eoi(irq_num: IntNumber) {
    let mut gic = GIC.lock();
    let gic = gic.get_mut().unwrap();
    gic.end_of_interrupt(irq_num);
}

#[rustfmt::skip]
impl fmt::Display for SpsrEL1 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Raw value.
        writeln!(f, "\rSPSR_EL1: {:#010x}", self.0.get())?;

        let to_flag_str = |x| -> _ { if x { "Set" } else { "Not set" } };

        writeln!(f, "\r      Flags:")?;
        writeln!(f, "\r            Negative (N): {}", to_flag_str(self.0.is_set(SPSR_EL1::N)))?;
        writeln!(f, "\r            Zero     (Z): {}", to_flag_str(self.0.is_set(SPSR_EL1::Z)))?;
        writeln!(f, "\r            Carry    (C): {}", to_flag_str(self.0.is_set(SPSR_EL1::C)))?;
        writeln!(f, "\r            Overflow (V): {}", to_flag_str(self.0.is_set(SPSR_EL1::V)))?;

        let to_mask_str = |x| -> _ { if x { "Masked" } else { "Unmasked" } };

        writeln!(f, "\r      Exception handling state:")?;
        writeln!(f, "\r            Debug  (D): {}", to_mask_str(self.0.is_set(SPSR_EL1::D)))?;
        writeln!(f, "\r            SError (A): {}", to_mask_str(self.0.is_set(SPSR_EL1::A)))?;
        writeln!(f, "\r            IRQ    (I): {}", to_mask_str(self.0.is_set(SPSR_EL1::I)))?;
        writeln!(f, "\r            FIQ    (F): {}", to_mask_str(self.0.is_set(SPSR_EL1::F)))?;

        write!(f, "\r      Illegal Execution State (IL): {}",
            to_flag_str(self.0.is_set(SPSR_EL1::IL))
        )
    }
}

#[rustfmt::skip]
impl fmt::Display for EsrEL1 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Raw print of whole register.
        writeln!(f, "\nESR_EL1: {:#010x}", self.0.get())?;

        // Raw print of exception class.
        writeln!(f, "\r      Exception Class         (EC) : {:#x}", self.0.read(ESR_EL1::EC))?;

        // Exception class.
        let ec_translation = match self.exception_class() {
            Some(ESR_EL1::EC::Value::DataAbortCurrentEL) => "Data Abort, current EL",
            _ => "N/A",
        };
        writeln!(f, "\r - {}", ec_translation)?;

        // Raw print of instruction specific syndrome.
        write!(f, "\r      Instr Specific Syndrome (ISS): {:#x}", self.0.read(ESR_EL1::ISS))
    }
}

impl fmt::Display for ExceptionContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "\r{}", self.esr_el1)?;

        if self.fault_address_valid() {
            writeln!(f, "\rFAR_EL1: {:#018x}", FAR_EL1.get() as usize)?;
        }

        writeln!(f, "\r{}", self.spsr_el1)?;
        writeln!(f, "\rELR_EL1: {:#018x}", self.elr_el1)?;
        writeln!(f)?;
        writeln!(f, "\rGeneral purpose register:")?;

        let alternating = |x| -> _ {
            if x % 2 == 0 { "   " } else { "\r" }
        };

        // Print two registers per line.
        for (i, reg) in self.gpr.iter().enumerate() {
            writeln!(f, "\r      x{: <2}: {: >#018x}{}", i, reg, alternating(i))?;
        }
        write!(f, "\r      lr : {:#018x}", self.lr)
    }
}

#[no_mangle]
extern "C" fn current_el0_synchronous(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

#[no_mangle]
extern "C" fn current_el0_irq(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

#[no_mangle]
extern "C" fn current_el0_serror(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

#[no_mangle]
extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e, "current_elx_synchronous");
}

#[no_mangle]
extern "C" fn current_elx_irq(exc: &mut ExceptionContext) {
    // read IRQ num
    // read IRQ priority
    // ackownledge IRQ to the GIC
    let (irq_num, _priority) = {
        let mut gic = GIC.lock();
        let gic = gic.get_mut().expect("GIC is uninitialized!");
        gic.acknowledge_int()
    };
    // important: GIC mutex is now implicitly unlocked

    let handler = IRQ_HANDLERS.read()[irq_num as usize];
    if !handler(exc) {
        // handler has returned, we can lock again
        let mut gic = GIC.lock();
        gic.get_mut().unwrap().end_of_interrupt(irq_num);
    }
}

#[no_mangle]
extern "C" fn current_elx_serror(e: &mut ExceptionContext) {
    default_exception_handler(e, "current_elx_serror");
}

#[no_mangle]
extern "C" fn lower_aarch64_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e, "lower_aarch64_synchronous");
}

#[no_mangle]
extern "C" fn lower_aarch64_irq(e: &mut ExceptionContext) {
    default_exception_handler(e, "lower_aarch64_irq");
}

#[no_mangle]
extern "C" fn lower_aarch64_serror(e: &mut ExceptionContext) {
    default_exception_handler(e, "lower_aarch64_serror");
}

#[no_mangle]
extern "C" fn lower_aarch32_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e, "lower_aarch32_synchronous");
}

#[no_mangle]
extern "C" fn lower_aarch32_irq(e: &mut ExceptionContext) {
    default_exception_handler(e, "lower_aarch32_irq");
}

#[no_mangle]
extern "C" fn lower_aarch32_serror(e: &mut ExceptionContext) {
    default_exception_handler(e, "lower_aarch32_serror");
}

impl EsrEL1 {
    #[inline(always)]
    fn exception_class(&self) -> Option<ESR_EL1::EC::Value> {
        self.0.read_as_enum(ESR_EL1::EC)
    }
}

impl ExceptionContext {
    #[inline(always)]
    fn exception_class(&self) -> Option<ESR_EL1::EC::Value> {
        self.esr_el1.exception_class()
    }

    #[inline(always)]
    fn fault_address_valid(&self) -> bool {
        use ESR_EL1::EC::Value::*;

        match self.exception_class() {
            None => false,
            Some(ec) => matches!(
                ec,
                InstrAbortLowerEL
                    | InstrAbortCurrentEL
                    | PCAlignmentFault
                    | DataAbortLowerEL
                    | DataAbortCurrentEL
                    | WatchpointLowerEL
                    | WatchpointCurrentEL
            ),
        }
    }
}
