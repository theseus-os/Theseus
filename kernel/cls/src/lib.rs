//! A library for defining CPU-local variables.
//!
//! See [`cpu_local`] for more details.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

pub use cls_macros::cpu_local;
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
    cpu: u32,
    _page: memory::AllocatedPages,
    used: usize,
}

// TODO: Store size of used CLS in gs:[0].
static CLS_SECTIONS: SpinMutex<Vec<CpuLocalDataRegion>> = SpinMutex::new(Vec::new());

pub fn init() {
    use core::arch::asm;

    let page = memory::allocate_pages(1).expect("couldn't allocate page for CLS section");
    let address = page.start_address().value();

    CLS_SECTIONS.lock().push(CpuLocalDataRegion {
        cpu: 0,
        _page: page,
        used: 0,
    });

    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm!(
            "wrgsbase {}",
            in(reg) address,
            options(nomem, preserves_flags, nostack),
        )
    };
    #[cfg(target_arch = "aarch64")]
    unsafe {
        asm!(
            "msr tpidr_el1, {}",
            in(reg) address,
            options(nomem, preserves_flags, nostack),
        )
    };
}

pub fn allocate(len: usize) -> usize {
    let cpu = cpu::current_cpu().value();

    let mut region_ref = None;
    let mut locked = CLS_SECTIONS.lock();

    for region in locked.iter_mut() {
        if region.cpu == cpu {
            region_ref = Some(region);
            break;
        }
    }

    let region_ref = region_ref.unwrap();
    let offset = region_ref.used;
    assert!(region_ref.used + len <= 4096);
    region_ref.used += len;

    offset
}
