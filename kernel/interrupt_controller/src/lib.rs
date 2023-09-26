#![no_std]
#![allow(unused_variables, unused_mut)]
#![feature(array_try_from_fn)]

extern crate alloc;

use alloc::vec::Vec;
use cpu::CpuId;

#[cfg(target_arch = "aarch64")]
#[path = "aarch64.rs"]
pub mod arch;

#[cfg(target_arch = "x86_64")]
#[path = "x86_64.rs"]
pub mod arch;

pub use arch::{
    SystemInterruptControllerVersion,
    SystemInterruptControllerId,
    LocalInterruptControllerId,
    Priority,
    SystemInterruptController,
    LocalInterruptController,
    init,
};

pub type InterruptNumber = u8;

/// The Cpu where this interrupt should be handled, as well as
/// the local interrupt number this gets translated to.
///
/// On aarch64, there is no `local_number` field as the system interrupt
/// number and the local interrupt number must be the same.
#[derive(Debug, Copy, Clone)]
pub enum InterruptDestination {
    SpecificCpu(CpuId),
    AllOtherCpus,
}

pub trait SystemInterruptControllerApi {
    fn get() -> &'static Self;

    fn id(&self) -> SystemInterruptControllerId;
    fn version(&self) -> SystemInterruptControllerVersion;

    fn get_destination(
        &self,
        interrupt_num: InterruptNumber,
    ) -> Result<(Vec<CpuId>, Priority), &'static str>;

    fn set_destination(
        &self,
        sys_int_num: InterruptNumber,
        destination: Option<CpuId>,
        priority: Priority,
    ) -> Result<(), &'static str>;
}

pub trait LocalInterruptControllerApi {
    fn get() -> &'static Self;

    fn id(&self) -> LocalInterruptControllerId;
    fn get_local_interrupt_priority(&self, num: InterruptNumber) -> Priority;
    fn set_local_interrupt_priority(&self, num: InterruptNumber, priority: Priority);
    fn is_local_interrupt_enabled(&self, num: InterruptNumber) -> bool;
    fn enable_local_interrupt(&self, num: InterruptNumber, enabled: bool);

    /// Sends an inter-processor interrupt.
    ///
    /// If `dest` is Some, the interrupt is sent to a specific CPU.
    /// If it's None, all CPUs except the sender receive the interrupt.
    fn send_ipi(&self, num: InterruptNumber, dest: InterruptDestination);

    /// Tell the interrupt controller that the current interrupt has been handled.
    fn end_of_interrupt(&self, number: InterruptNumber);
}

/// AArch64-specific methods of a local interrupt controller
pub trait AArch64LocalInterruptControllerApi {
    /// Same as [`LocalInterruptControllerApi::enable_local_interrupt`] but for fast interrupts (FIQs).
    fn enable_fast_local_interrupt(&self, num: InterruptNumber, enabled: bool);

    /// Same as [`LocalInterruptControllerApi::send_ipi`] but for fast interrupts (FIQs).
    fn send_fast_ipi(&self, num: InterruptNumber, dest: InterruptDestination);

    /// Reads the minimum priority for an interrupt to reach this CPU.
    fn get_minimum_priority(&self) -> Priority;

    /// Changes the minimum priority for an interrupt to reach this CPU.
    fn set_minimum_priority(&self, priority: Priority);

    /// Aarch64-specific way to read the current pending interrupt number & priority.
    fn acknowledge_interrupt(&self) -> Option<(InterruptNumber, Priority)>;

    /// Aarch64-specific way to initialize the secondary CPU interfaces.
    ///
    /// Must be called once from every secondary CPU.
    fn init_secondary_cpu_interface(&self);

    /// Same as [`Self::acknowledge_interrupt`] but for fast interrupts (FIQs)
    ///
    /// # Safety
    ///
    /// This is unsafe because it circumvents the internal Mutex.
    /// It must only be used by the `interrupts` crate when handling an FIQ.
    unsafe fn acknowledge_fast_interrupt(&self) -> Option<(InterruptNumber, Priority)>;

    /// Same as [`LocalInterruptControllerApi::end_of_interrupt`] but for fast interrupts (FIQs)
    ///
    /// # Safety
    ///
    /// This is unsafe because it circumvents the internal Mutex.
    /// It must only be used by the `interrupts` crate when handling an FIQ.
    unsafe fn end_of_fast_interrupt(&self, number: InterruptNumber);
}
