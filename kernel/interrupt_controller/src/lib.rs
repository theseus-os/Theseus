//! Support for accessing interupt controllers across multiple architectures.

#![no_std]
#![allow(unused_variables, unused_mut)]
#![feature(array_try_from_fn)]

extern crate alloc;

use alloc::vec::Vec;
use cpu::CpuId;
use memory::MmiRef;

#[cfg_attr(target_arch = "x86_64", path = "x86_64.rs")]
#[cfg_attr(target_arch = "aarch64", path = "aarch64.rs")]
mod arch;

pub use arch::{
    SystemInterruptControllerVersion,
    SystemInterruptControllerId,
    LocalInterruptControllerId,
    Priority,
    SystemInterruptController,
    LocalInterruptController,
};

#[cfg(target_arch = "aarch64")]
pub use arch::AArch64LocalInterruptControllerApi;

pub type InterruptNumber = u8;


/// Initializes the interrupt controller(s) on this system.
///
/// Depending on the architecture, this includes both the system-wide
/// interrupt controller(s) and one or more local (per-CPU) interrupt controllers.
/// * On x86_64 systems, this initializes the I/O APIC(s) (there may be more than one)
///   as the system-wide interrupt controller(s), and the Local APIC for the BSP
///   (bootstrap processor) only as the first local interrupt controller.
///   * Other Local APICs are initialized by those CPUs when they are brought online.
/// * On aarch64 systems with GIC, this initializes both the system-wide
///   interrupt controller (the GIC Distributor) as well as the local controllers
///   for all CPUs (their Redistributors and CPU interfaces).
pub fn init(kernel_mmi: &MmiRef) -> Result<(), &'static str> {
    arch::init(kernel_mmi)
}

/// The CPU where an interrupt should be handled, as well as
/// the local interrupt number this gets translated to.
///
/// On aarch64, there is no `local_number` field as the system interrupt
/// number and the local interrupt number must be the same.
#[derive(Debug, Copy, Clone)]
pub enum InterruptDestination {
    SpecificCpu(CpuId),
    AllOtherCpus,
}

/// Functionality provided by system-wide interrupt controllers.
///
/// Note that a system may actually have *multiple* system-wide interrupt controllers.
///
/// * On x86_64, this corresponds to an I/O APIC (IOAPIC).
/// * On aarch64 (with GIC), this corresponds to the Distributor.
pub trait SystemInterruptControllerApi {
    /// Returns the unique ID of this system-wide interrupt controller.
    fn id(&self) -> SystemInterruptControllerId;

    /// Returns the version ID of this system-wide interrupt controller.
    fn version(&self) -> SystemInterruptControllerVersion;

    /// Returns the destination(s) that the given `interrupt` is routed to
    /// by this system-wide interrupt controller.
    fn get_destination(
        &self,
        interrupt: InterruptNumber,
    ) -> Result<(Vec<CpuId>, Priority), &'static str>;
    
    /// Routes the given `interrupt` to the given `destination` with the given `priority`. 
    fn set_destination(
        &self,
        interrupt: InterruptNumber,
        destination: Option<CpuId>,
        priority: Priority,
    ) -> Result<(), &'static str>;
}

/// Functionality provided by local interrupt controllers,
/// which exist on a per-CPU basis.
///
/// * On x86_64, this corresponds to a Local APIC.
/// * On aarch64 (with GIC), this corresponds to the GIC Redistributor + CPU interface.
pub trait LocalInterruptControllerApi {
    /// Returns the unique ID of this local interrupt controller.
    fn id(&self) -> LocalInterruptControllerId;
    
    /// Enables or disables the local timer interrupt for this local interrupt controller.
    fn enable_local_timer_interrupt(&self, enable: bool);

    /// Sends an inter-processor interrupt from this local interrupt controller
    /// to the given destination.
    fn send_ipi(&self, num: InterruptNumber, dest: InterruptDestination);

    /// Tells this local interrupt controller that the interrupt being currently serviced
    /// has been completely handled.
    fn end_of_interrupt(&self, number: InterruptNumber);
}
