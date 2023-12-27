use core::arch::global_asm;
use core::fmt;

use crate::EoiBehaviour;

use cortex_a::registers::*;

use tock_registers::interfaces::Writeable;
use tock_registers::interfaces::Readable;
use tock_registers::registers::InMemoryRegister;

use interrupt_controller::{
    LocalInterruptController, SystemInterruptController, InterruptDestination,
    LocalInterruptControllerApi, AArch64LocalInterruptControllerApi, SystemInterruptControllerApi,
};
use arm_boards::BOARD_CONFIG;
use sync_irq::IrqSafeRwLock;
use cpu::current_cpu;
use log::*;

pub use interrupt_controller::InterruptNumber;

// This assembly file contains trampolines to `extern "C"` functions defined below.
global_asm!(include_str!("table.s"));

/// The IRQ number reserved for the PL011 Single-Serial-Port Controller
/// which Theseus currently uses for logging and UART console.
pub const PL011_RX_SPI: InterruptNumber = BOARD_CONFIG.pl011_rx_spi;

/// The IRQ number reserved for CPU-local timer interrupts,
/// which Theseus currently uses for preemptive task switching.
pub const CPU_LOCAL_TIMER_IRQ: InterruptNumber = BOARD_CONFIG.cpu_local_timer_ppi;

/// The IRQ/IPI number for TLB Shootdowns
///
/// Note: This is arbitrarily defined in the range 0..16,
/// which is reserved for IPIs (SGIs - for software generated
/// interrupts - in GIC terminology).
pub const TLB_SHOOTDOWN_IPI: InterruptNumber = 2;

const MAX_IRQ_NUM: usize = 256;

// Singleton which acts like an x86-style Interrupt Descriptor Table:
// it's an array of function pointers which are meant to handle IRQs.
// Synchronous Exceptions (including syscalls) are not IRQs on aarch64;
// this crate doesn't expose any way to handle them at the moment.
static IRQ_HANDLERS: IrqSafeRwLock<[Option<InterruptHandler>; MAX_IRQ_NUM]> = IrqSafeRwLock::new([None; MAX_IRQ_NUM]);

/// The Saved Program Status Register at the time of the exception.
#[repr(transparent)]
struct SpsrEL1(InMemoryRegister<u64, SPSR_EL1::Register>);

/// The Exception Syndrome Register at the time of the exception.
#[repr(transparent)]
struct EsrEL1(InMemoryRegister<u64, ESR_EL1::Register>);

#[macro_export]
#[doc = include_str!("../macro-doc.md")]
macro_rules! interrupt_handler {
    ($name:ident, _, $stack_frame:ident, $code:block) => {
        interrupt_handler!($name, 0, $stack_frame, $code);
    };
    ($name:ident, $x86_64_eoi_param:expr, $stack_frame:ident, $code:block) => {
        extern "C" fn $name($stack_frame: &$crate::InterruptStackFrame) -> $crate::EoiBehaviour $code
    };
}

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

pub type InterruptHandler = extern "C" fn(&InterruptStackFrame) -> EoiBehaviour;
pub type InterruptStackFrame = ExceptionContext;

// called for all exceptions other than interrupts
fn default_exception_handler(exc: &ExceptionContext, origin: &'static str) {
    log::error!("Unhandled Exception ({})\r\n{:?}\r\n[looping forever now]", origin, exc);
    loop { core::hint::spin_loop() }
}

/// Sets `VBAR_EL1` to the start of the exception vector
fn set_vbar_el1() {
    extern "Rust" {
        // in assembly file
        static __exception_vector_start: extern "C" fn();
    }

    // Set the exception handling vector, which
    // is an array of grouped aarch64 instructions.
    // see table.s for more info.
    unsafe { VBAR_EL1.set(&__exception_vector_start as *const _ as u64) };
}

/// Sets `VBAR_EL1` to the start of the exception vector
/// and enables timer interrupts
pub fn init_ap() {
    set_vbar_el1();

    let int_ctrl = LocalInterruptController::get()
        .expect("LocalInterruptController was not yet initialized");
    int_ctrl.init_secondary_cpu_interface();
    int_ctrl.set_minimum_priority(0);

    // Enable the TLB shootdown IPI to be delivered to this CPU.
    // On the bootstrap CPU, this is done in `setup_tlb_shootdown_handler()`.
    int_ctrl.enable_fast_local_interrupt(TLB_SHOOTDOWN_IPI, true);

    // Enable the CPU-local timer interrupt to be delivered to this CPU.
    // On the bootstrap CPU, this is done in `setup_timer_interrupt()`.
    int_ctrl.enable_local_interrupt(CPU_LOCAL_TIMER_IRQ, true);

    generic_timer_aarch64::enable_timer_interrupt(true);
}

/// Initializes the generic system timer and the system-wide list of interrupt handlers.
///
/// This only needs to be invoked once, system-wide.
pub fn init() -> Result<(), &'static str> {
    generic_timer_aarch64::init();
    set_vbar_el1();

    // TODO: see note in captain::init(): just call interrupt_controller::init() here directly.

    let int_ctrl = LocalInterruptController::get()
        .expect("LocalInterruptController was not yet initialized");
    int_ctrl.set_minimum_priority(0);
    Ok(())
}

/// Registers an interrupt handler for the CPU-local timer
/// and handles interrupt controller configuration for that timer interrupt.
pub fn setup_timer_interrupt(timer_tick_handler: InterruptHandler) -> Result<(), &'static str> {
    // register/deregister the handler for the timer IRQ.
    if let Err(existing_handler) = register_interrupt(CPU_LOCAL_TIMER_IRQ, timer_tick_handler) {
        if timer_tick_handler as InterruptHandler != existing_handler {
            return Err("A different interrupt handler has already been setup for the timer IRQ number");
        }
    }

    // Route the IRQ to this core (implicit as IRQ < 32) & Enable the interrupt.
    {
        let int_ctrl = LocalInterruptController::get()
            .ok_or("LocalInterruptController was not yet initialized")?;

        // enable routing of this interrupt
        int_ctrl.enable_local_interrupt(CPU_LOCAL_TIMER_IRQ, true);
    }

    Ok(())
}

/// This function registers an interrupt handler for an inter-processor interrupt
/// and handles interrupt controller configuration for that interrupt.
///
/// Returns an error if the specified interrupt number already has a registered handler.
pub fn setup_ipi_handler(handler: InterruptHandler, local_num: InterruptNumber) -> Result<(), &'static str> {
    // register the handler
    if let Err(existing_handler) = register_interrupt(local_num, handler) {
        if handler as InterruptHandler != existing_handler {
            return Err("A different interrupt handler has already been setup for that IPI");
        }
    }

    {
        let int_ctrl = LocalInterruptController::get()
            .ok_or("LocalInterruptController was not yet initialized")?;
        // enable routing of this interrupt
        int_ctrl.enable_local_interrupt(local_num, true);
    }

    Ok(())
}

/// This function registers an interrupt handler for the TLB Shootdown IPI
/// and handles interrupt controller configuration for that interrupt.
///
/// Returns an error if the TLB Shootdown interrupt number already has a registered handler.
pub fn setup_tlb_shootdown_handler(handler: InterruptHandler) -> Result<(), &'static str> {
    if let Err(existing_handler) = register_interrupt(TLB_SHOOTDOWN_IPI, handler) {
        if handler as InterruptHandler != existing_handler {
            return Err("A different interrupt handler has already been setup for that IPI");
        }
    }

    {
        // enable this interrupt as a Fast interrupt (FIQ / Group 0 interrupt)
        let int_ctrl = LocalInterruptController::get()
            .ok_or("LocalInterruptController was not yet initialized")?;
        int_ctrl.enable_fast_local_interrupt(TLB_SHOOTDOWN_IPI, true);
    }

    Ok(())
}

/// Enables the PL011 receive interrupt ("RX" SPI) and routes it to the current CPU.
pub fn init_pl011_rx_interrupt() -> Result<(), &'static str> {
    let int_ctrl = SystemInterruptController::get()
        .ok_or("SystemInterruptController was not yet initialized")?;
    int_ctrl.set_destination(PL011_RX_SPI, Some(current_cpu()), u8::MAX)
}

/// Registers an interrupt handler at the given IRQ interrupt number.
///
/// The function fails if the interrupt number is reserved or is already in use.
///
/// # Arguments 
/// * `int_num`: the interrupt number that is being requested.
/// * `func`: the handler to be registered, which will be invoked when the interrupt occurs.
///
/// # Return
/// * `Ok(())` if successfully registered, or
/// * `Err(existing_handler_address)` if the given `irq_num` was already in use.
pub fn register_interrupt(int_num: InterruptNumber, func: InterruptHandler) -> Result<(), InterruptHandler> {
    let mut handlers = IRQ_HANDLERS.write();
    let index = int_num as usize;

    if let Some(handler) = handlers[index] {
        error!("register_interrupt: the requested interrupt IRQ {} was already in use", index);
        Err(handler)
    } else {
        handlers[index] = Some(func);
        Ok(())
    }
}

/// Deregisters an interrupt handler, making it available to the rest of the system again.
///
/// As a sanity/safety check, the caller must provide the `interrupt_handler`
/// that is currently registered for the given IRQ `interrupt_num`.
/// This function returns an error if the currently-registered handler does not match 'func'.
///
/// # Arguments
/// * `int_num`: the interrupt number that needs to be deregistered
/// * `func`: the handler that should currently be stored for 'interrupt_num'
pub fn deregister_interrupt(int_num: InterruptNumber, func: InterruptHandler) -> Result<(), Option<InterruptHandler>> {
    let mut handlers = IRQ_HANDLERS.write();
    let index = int_num as usize;

    let func = func as InterruptHandler;
    let handler = handlers[index].map(|h| h as InterruptHandler);

    if handler != Some(func) {
        error!("deregister_interrupt: Cannot free interrupt due to incorrect handler function");
        Err(handler)
    } else {
        handlers[index] = None;
        Ok(())
    }
}

/// Broadcast an Inter-Processor Interrupt to all other CPU cores in the system
pub fn broadcast_ipi(ipi_num: InterruptNumber) {
    let int_ctrl = LocalInterruptController::get()
        .expect("LocalInterruptController was not yet initialized");
    int_ctrl.send_ipi(ipi_num, InterruptDestination::AllOtherCpus);
}

/// Broadcast the TLB Shootdown Inter-Processor Interrupt to all other
/// CPU cores in the system
///
/// This IPI uses fast interrupts (FIQs) as an NMI alternative.
pub fn broadcast_tlb_shootdown_ipi() {
    let int_ctrl = LocalInterruptController::get()
        .expect("LocalInterruptController was not yet initialized");
    int_ctrl.send_fast_ipi(TLB_SHOOTDOWN_IPI, InterruptDestination::AllOtherCpus);
}

/// Send an "end of interrupt" signal, notifying the interrupt chip that
/// the given interrupt request `irq` has been serviced.
pub fn eoi(irq_num: InterruptNumber) {
    let int_ctrl = LocalInterruptController::get()
        .expect("LocalInterruptController was not yet initialized");
    int_ctrl.end_of_interrupt(irq_num);
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
        writeln!(f, "\r - {ec_translation}")?;

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

// When this is entered, FIQs are enabled / unmasked, because we use
// them as an NMI alternative, so they must be allowed at all times.
//
// Spurious interrupts are often the result of an FIQ being handled
// after we started handling an IRQ but before we acknowledged it.
#[no_mangle]
extern "C" fn current_elx_irq(exc: &mut ExceptionContext) {
    let (irq_num, _priority) = {
        let int_ctrl = LocalInterruptController::get()
            .expect("LocalInterruptController was not yet initialized");
        match int_ctrl.acknowledge_interrupt() {
            Some(irq_prio_tuple) => irq_prio_tuple,
            None /* spurious interrupt */ => return,
        }
    };

    let index = irq_num as usize;
    let handler = IRQ_HANDLERS.read().get(index).copied().flatten();
    let result = handler.map(|handler| handler(exc));

    if let Some(result) = result {
        if result == EoiBehaviour::HandlerDidNotSendEoi {
            // will use LocalInterruptController
            eoi(irq_num);
        }
    } else {
        log::error!("Unhandled IRQ: {}\r\n{:?}\r\n[looping forever now]", irq_num, exc);
        loop { core::hint::spin_loop() }
    }
}

// When this is entered, FIQs are disabled / masked: there must be
// only one FIQ (that we use as an NMI alternative) at a time.
//
// Currently, FIQs are only used for TLB shootdown.
#[no_mangle]
extern "C" fn current_elx_fiq(exc: &mut ExceptionContext) {
    let (irq_num, _priority) = {
        let int_ctrl = LocalInterruptController::get()
            .expect("LocalInterruptController was not yet initialized");
        let ack = unsafe { int_ctrl.acknowledge_fast_interrupt() };
        match ack {
            Some(irq_prio_tuple) => irq_prio_tuple,
            None /* spurious interrupt */ => return,
        }
    };

    let handler = IRQ_HANDLERS.read().get(irq_num as usize).copied().flatten();
    let result = handler.map(|handler| handler(exc));

    if let Some(result) = result {
        if result == EoiBehaviour::HandlerDidNotSendEoi {
            let int_ctrl = LocalInterruptController::get()
                .expect("LocalInterruptController was not yet initialized");
            unsafe { int_ctrl.end_of_fast_interrupt(irq_num) };
        }
    } else {
        log::error!("Unhandled FIQ: {}\r\n{:?}\r\n[looping forever now]", irq_num, exc);
        loop { core::hint::spin_loop() }
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
