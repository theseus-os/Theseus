//! A library for defining CPU-local variables.
//!
//! See [`cpu_local`] for more details.

#![feature(int_roundings)]
#![no_std]

extern crate alloc;

use alloc::vec::Vec;

pub use cls_macros::cpu_local;
use cpu::CpuId;
use memory::PteFlags;
use sync_spin::SpinMutex;

/// A trait abstracting over guards that ensure atomicity with respect to the
/// current CPU.
///
/// This trait is "sealed" and cannot be implemented by anything outside this
/// crate.
pub trait CpuAtomicGuard: sealed::Sealed {}

impl sealed::Sealed for irq_safety::HeldInterrupts {}
impl CpuAtomicGuard for irq_safety::HeldInterrupts {}

impl sealed::Sealed for preemption::PreemptionGuard {}
impl CpuAtomicGuard for preemption::PreemptionGuard {}

mod sealed {
    pub trait Sealed {}
}

#[doc(hidden)]
pub mod __private {
    #[cfg(target_arch = "aarch64")]
    pub use cortex_a;
    pub use preemption;
    #[cfg(target_arch = "aarch64")]
    pub use tock_registers;
    #[cfg(target_arch = "x86_64")]
    pub use x86_64;
}

use tls_initializer::{ClsDataImage, ClsInitializer};

static CLS_INITIALIZER: SpinMutex<ClsInitializer> = SpinMutex::new(ClsInitializer::new());
static CLS_SECTIONS: SpinMutex<Vec<(CpuId, ClsDataImage)>> = SpinMutex::new(Vec::new());

/// Adds a CLS section with a pre-determined offset to the global CLS
/// initializer.
///
/// The CLS register will not be updated until either [`reload`] or
/// [`reload_current_core`] is called.
pub fn add_static_section() {
    todo!();
}

/// Adds a dynamic CLS section to the global CLS initializer.
///
/// The CLS register will not be updated until either [`reload`] or
/// [`reload_current_core`] is called.
pub fn add_dynamic_section() {
    todo!();
}

/// Generates a new data image for the current core and sets the CLS register
/// accordingly.
pub fn reload_current_core() {
    let current_cpu = cpu::current_cpu();

    let mut data = CLS_INITIALIZER.lock().get_data();
    // SAFETY: TODO
    unsafe { data.set_as_current_cls() };

    let mut sections = CLS_SECTIONS.lock();
    for (cpu, image) in sections.iter_mut() {
        if *cpu == current_cpu {
            core::mem::swap(image, &mut data);
            return;
        }
    }
    sections.push((current_cpu, data));
}

pub fn reload() {
    let _initializer = CLS_INITIALIZER.lock();
    // FIXME: Reload CLS register on all cores.
}
