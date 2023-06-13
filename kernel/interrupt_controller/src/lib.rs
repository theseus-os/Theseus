#![no_std]
#![allow(unused_variables, unused_mut)]

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
    SystemInterruptNumber,
    LocalInterruptNumber,
    Priority,
    SystemInterruptController,
    LocalInterruptController,
    init,
};

#[derive(Debug, Copy, Clone)]
pub enum InterruptNumber {
    Local(LocalInterruptNumber),
    System(SystemInterruptNumber),
}

/// The Cpu where this interrupt should be handled, as well as
/// the local interrupt number this gets translated to.
///
/// On aarch64, there is no `local_number` field as the system interrupt
/// number and the local interrupt number must be the same.
#[derive(Debug, Copy, Clone)]
pub struct InterruptDestination {
    pub cpu: CpuId,
    #[cfg(target_arch = "x86_64")]
    pub local_number: LocalInterruptNumber,
}

pub trait SystemInterruptControllerApi {
    fn id(&self) -> SystemInterruptControllerId;
    fn version(&self) -> SystemInterruptControllerVersion;

    fn get_destination(
        &self,
        interrupt_num: SystemInterruptNumber,
    ) -> Result<(Vec<InterruptDestination>, Priority), &'static str>;

    fn set_destination(
        &self,
        sys_int_num: SystemInterruptNumber,
        destination: InterruptDestination,
        priority: Priority,
    ) -> Result<(), &'static str>;
}

pub trait LocalInterruptControllerApi {
    /// Aarch64-specific way to initialize the secondary CPU interfaces.
    ///
    /// Must be called once from every secondary CPU.
    ///
    /// Always panics on x86_64.
    fn init_secondary_cpu_interface(&self);

    fn id(&self) -> LocalInterruptControllerId;
    fn get_local_interrupt_priority(&self, num: LocalInterruptNumber) -> Priority;
    fn set_local_interrupt_priority(&self, num: LocalInterruptNumber, priority: Priority);
    fn is_local_interrupt_enabled(&self, num: LocalInterruptNumber) -> bool;
    fn enable_local_interrupt(&self, num: LocalInterruptNumber, enabled: bool);

    /// Sends an inter-processor interrupt.
    ///
    /// If `dest` is Some, the interrupt is sent to a specific CPU.
    /// If it's None, all CPUs except the sender receive the interrupt.
    fn send_ipi(&self, num: LocalInterruptNumber, dest: Option<CpuId>);

    /// Reads the minimum priority for an interrupt to reach this CPU.
    ///
    /// Note: aarch64-only, at the moment.
    fn get_minimum_priority(&self) -> Priority;

    /// Changes the minimum priority for an interrupt to reach this CPU.
    ///
    /// Note: aarch64-only, at the moment.
    fn set_minimum_priority(&self, priority: Priority);

    /// Aarch64-specific way to read the current pending interrupt number & priority.
    ///
    /// Always panics on x86_64.
    fn acknowledge_interrupt(&self) -> (InterruptNumber, Priority);

    /// Tell the interrupt controller that the current interrupt has been handled.
    fn end_of_interrupt(&self, number: InterruptNumber);
}

#[cfg(target_arch = "aarch64")]
impl From<InterruptNumber> for usize {
    fn from(num: InterruptNumber) -> usize {
        match num {
            InterruptNumber::Local(n) => n.0 as usize,
            InterruptNumber::System(n) => n.0 as usize,
        }
    }
}

impl From<LocalInterruptNumber> for InterruptNumber {
    fn from(num: LocalInterruptNumber) -> InterruptNumber {
        InterruptNumber::Local(num)
    }
}

impl From<SystemInterruptNumber> for InterruptNumber {
    fn from(num: SystemInterruptNumber) -> InterruptNumber {
        InterruptNumber::System(num)
    }
}
