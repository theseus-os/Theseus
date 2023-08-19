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

struct CpuLocalDataRegion {
    cpu: CpuId,
    image: TlsDataImage,
}

use tls_initializer::TlsDataImage;

// TODO: Store pointer to image in gs:[0]?
static CLS_SECTIONS: SpinMutex<Vec<TlsDataImage>> = SpinMutex::new(Vec::new());

pub fn insert(image: TlsDataImage) {
    log::info!("CALLING INSERT");
    log::info!("ptr: {:0x?}", image.ptr);
    log::info!("data: {:0x?}", image._data);
    image.set_as_current_cls_base();
    let temp: u64;
    unsafe {
        ::core::arch::asm!("rdgsbase {}", out(reg) temp);
    }
    log::error!("gs: {temp:0x?}");
    CLS_SECTIONS.lock().push(image);
}
