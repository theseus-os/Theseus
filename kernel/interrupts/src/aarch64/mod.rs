use core::arch::global_asm;
use core::ops::DerefMut;
use core::fmt;

use cortex_a::registers::*;

use tock_registers::interfaces::Writeable;
use tock_registers::interfaces::Readable;
use tock_registers::registers::InMemoryRegister;

use gic::{qemu_virt_addrs, ArmGic, InterruptNumber, Version as GicVersion};
use irq_safety::{RwLockIrqSafe, MutexIrqSafe};
use memory::get_kernel_mmi_ref;
use log::{info, error};

use time::{Monotonic, ClockSource, Instant, Period, register_clock_source};

// This assembly file contains trampolines to `extern "C"` functions defined below.
global_asm!(include_str!("table.s"));

// The global Generic Interrupt Controller singleton
static GIC: MutexIrqSafe<Option<ArmGic>> = MutexIrqSafe::new(None);

/// The IRQ number reserved for CPU-local timer interrupts,
/// which Theseus currently uses for preemptive task switching.
//
// aarch64 manuals define the default timer IRQ number to be 30.
pub const CPU_LOCAL_TIMER_IRQ: InterruptNumber = 30;

const MAX_IRQ_NUM: usize = 256;

// Singleton which acts like an x86-style Interrupt Descriptor Table:
// it's an array of function pointers which are meant to handle IRQs.
// Synchronous Exceptions (including syscalls) are not IRQs on aarch64;
// this crate doesn't expose any way to handle them at the moment.
static IRQ_HANDLERS: RwLockIrqSafe<[HandlerFunc; MAX_IRQ_NUM]> = RwLockIrqSafe::new([default_irq_handler; MAX_IRQ_NUM]);

/// The Saved Program Status Register at the time of the exception.
#[repr(transparent)]
struct SpsrEL1(InMemoryRegister<u64, SPSR_EL1::Register>);

/// The Exception Syndrome Register at the time of the exception.
#[repr(transparent)]
struct EsrEL1(InMemoryRegister<u64, ESR_EL1::Register>);

/// The exception context as it is stored on the stack on exception entry.
///
/// Warning: `table.s` assumes this exact layout. If you modify this,
/// make sure to adapt the assembly code accordingly.
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

#[derive(Debug, PartialEq, Eq)]
pub enum EoiBehaviour {
    CallerMustSignalEoi,
    HandlerHasSignaledEoi,
}

/// Return value:
/// - true if you sent an End Of Interrupt signal in the handler
/// - false if you want the caller to do it for you after you return
type HandlerFunc = extern "C" fn(&ExceptionContext) -> EoiBehaviour;

// called for all exceptions other than interrupts
fn default_exception_handler(exc: &ExceptionContext, origin: &'static str) {
    log::error!("Unhandled Exception ({})\r\n{:?}\r\n[looping forever now]", origin, exc);
    loop {}
}

// called for all unhandled interrupt requests
extern "C" fn default_irq_handler(exc: &ExceptionContext) -> EoiBehaviour {
    log::error!("Unhandled IRQ:\r\n{:?}\r\n[looping forever now]", exc);
    loop {}
}

/// Please call this (only once) before using this crate.
///
/// This initializes the Generic Interrupt Controller
/// using the addresses which are valid on qemu's "virt" VM.
pub fn init() -> Result<(), &'static str> {
    extern "Rust" {
        // in assembly file
        static __exception_vector_start: extern "C" fn();
    }

    let counter_freq_hz = CNTFRQ_EL0.get() as f64;
    let fs_in_one_sec = 1_000_000_000_000_000.0;

    // https://doc.rust-lang.org/reference/expressions/operator-expr.html
    // "Casting from a float to an integer will round the float towards zero"
    let period_femtoseconds = (fs_in_one_sec / counter_freq_hz) as u64;

    register_clock_source::<PhysicalSystemCounter>(Period::new(period_femtoseconds));

    let mut gic = GIC.lock();
    if gic.is_some() {
        Err("The GIC has already been initialized!")
    } else {
        // Set the exception handling vector, which
        // is an array of grouped aarch64 instructions.
        // see table.s for more info.
        unsafe { VBAR_EL1.set(&__exception_vector_start as *const _ as u64) };

        let kernel_mmi_ref = get_kernel_mmi_ref()
            .ok_or("logger_aarch64: couldn't get kernel MMI ref")?;

        let mut mmi = kernel_mmi_ref.lock();
        let page_table = &mut mmi.deref_mut().page_table;

        log::info!("Configuring the GIC");
        let mut inner = ArmGic::init(
            page_table,
            GicVersion::InitV3 {
                dist: qemu_virt_addrs::GICD,
                redist: qemu_virt_addrs::GICR,
            },
        )?;

        inner.set_minimum_priority(0);
        *gic = Some(inner);

        log::info!("Done Configuring the GIC");

        Ok(())
    }
}

/// This function registers an interrupt handler for the CPU-local
/// timer, enables the routing of this interrupt in the GIC, and
/// turns the timer on, when `enable` is true.
///
/// When `enable` is false, the handler is deregistered, routing is
/// disabled in the GIC, and the timer is turned off.
pub fn enable_timer_interrupts(enable: bool, timer_tick_handler: HandlerFunc) -> Result<(), &'static str> {
    // register/deregister the handler for the timer IRQ.
    if enable {
        if let Err(existing_handler) = register_interrupt(CPU_LOCAL_TIMER_IRQ, timer_tick_handler) {
            if timer_tick_handler as *const HandlerFunc != existing_handler {
                return Err("A different interrupt handler has already been setup for the timer IRQ number");
            }
        }
    } else {
        if let Err(_existing_handler) = deregister_interrupt(CPU_LOCAL_TIMER_IRQ, timer_tick_handler) {
            return Err("A different interrupt handler was setup for the timer IRQ number");
        }
    }

    // Route the IRQ to this core (implicit as IRQ < 32) & Enable the interrupt.
    {
        let mut gic = GIC.lock();
        let gic = gic.as_mut().ok_or("GIC is uninitialized")?;

        // enable routing of this interrupt
        gic.set_interrupt_state(CPU_LOCAL_TIMER_IRQ, enable);
    }

    // unmask the interrupt & enable the timer
    CNTP_CTL_EL0.write(
          CNTP_CTL_EL0::IMASK.val(0)
        + CNTP_CTL_EL0::ENABLE.val(match enable {
            true => 1,
            false => 0,
        })
    );

    /* DEBUGGING CODE

    log::info!("timer enabled: {:?}",  CNTP_CTL_EL0.read(CNTP_CTL_EL0::ENABLE));
    log::info!("timer IMASK: {:?}",   CNTP_CTL_EL0.read(CNTP_CTL_EL0::IMASK));
    log::info!("timer status: {:?}", CNTP_CTL_EL0.read(CNTP_CTL_EL0::ISTATUS));

    */

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
pub fn register_interrupt(irq_num: InterruptNumber, func: HandlerFunc) -> Result<(), *const HandlerFunc> {
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
pub fn deregister_interrupt(irq_num: InterruptNumber, func: HandlerFunc) -> Result<(), *const HandlerFunc> {
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
pub fn eoi(irq_num: InterruptNumber) {
    let mut gic = GIC.lock();
    let gic = gic.as_mut().expect("GIC is uninitialized");
    gic.end_of_interrupt(irq_num);
}

// A ClockSource for the time crate, implemented using
// the System Counter of the Generic Arm Timer. The
// period of this timer is computed in `init` above.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct PhysicalSystemCounter;

impl ClockSource for PhysicalSystemCounter {
    type ClockType = Monotonic;

    fn now() -> Instant {
        Instant::new(CNTPCT_EL0.get())
    }
}

#[rustfmt::skip]
impl fmt::Debug for SpsrEL1 {
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
impl fmt::Debug for EsrEL1 {
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

impl fmt::Debug for ExceptionContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "\r{:?}", self.esr_el1)?;

        if self.fault_address_valid() {
            writeln!(f, "\rFAR_EL1: {:#018x}", FAR_EL1.get() as usize)?;
        }

        writeln!(f, "\r{:?}", self.spsr_el1)?;
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
    panic!("BUG: Use of SP_EL0 in EL1 is not supported.")
}

#[no_mangle]
extern "C" fn current_el0_irq(_e: &mut ExceptionContext) {
    panic!("BUG: Use of SP_EL0 in EL1 is not supported.")
}

#[no_mangle]
extern "C" fn current_el0_serror(_e: &mut ExceptionContext) {
    panic!("BUG: Use of SP_EL0 in EL1 is not supported.")
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
        let gic = gic.as_mut().expect("GIC is uninitialized");
        gic.acknowledge_interrupt()
    };
    // important: GIC mutex is now implicitly unlocked
    let irq_num_usize = irq_num as usize;

    let handler = match irq_num_usize < MAX_IRQ_NUM {
        true => IRQ_HANDLERS.read()[irq_num_usize],
        false => default_irq_handler,
    };

    if handler(exc) == EoiBehaviour::CallerMustSignalEoi {
        // handler has returned, we can lock again
        let mut gic = GIC.lock();
        gic.as_mut().unwrap().end_of_interrupt(irq_num);
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
