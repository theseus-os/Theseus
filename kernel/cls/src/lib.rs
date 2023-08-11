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
    _mapped_page: memory::MappedPages,
    used: usize,
}

// TODO: Store size of used CLS in gs:[0].
static CLS_SECTIONS: SpinMutex<Vec<CpuLocalDataRegion>> = SpinMutex::new(Vec::new());

pub fn init(cpu: CpuId) {
    use core::arch::asm;
    log::info!("a");

    let page = memory::allocate_pages(1).expect("couldn't allocate page for CLS section");
    log::info!("b");
    let address = page.start_address().value();
    log::info!("c");
    log::error!("(cpu {cpu:?}) allocated page: {page:?}");
    let mapped_page = memory::get_kernel_mmi_ref()
        .unwrap()
        .lock()
        .page_table
        .map_allocated_pages(page, PteFlags::VALID | PteFlags::WRITABLE)
        .unwrap();

    CLS_SECTIONS.lock().push(CpuLocalDataRegion {
        cpu,
        _mapped_page: mapped_page,
        used: 0,
    });
    log::info!("d");

    #[cfg(target_arch = "x86_64")]
    {
        use x86_64::registers::control::{Cr4, Cr4Flags};
        unsafe { Cr4::update(|flags| flags.insert(Cr4Flags::FSGSBASE)) };

        unsafe {
            asm!(
                "wrgsbase {}",
                in(reg) address,
                options(nomem, preserves_flags, nostack),
            )
        }
    };
    #[cfg(target_arch = "aarch64")]
    unsafe {
        asm!(
            "msr tpidr_el1, {}",
            in(reg) address,
            options(nomem, preserves_flags, nostack),
        )
    };
    log::info!("done init");
}

pub fn allocate(len: usize, alignment: usize) -> usize {
    log::info!("start alloc");
    let cpu = cpu::current_cpu();

    let mut region_ref: Option<&mut CpuLocalDataRegion> = None;
    let mut locked = CLS_SECTIONS.lock();

    for region in locked.iter_mut() {
        if region.cpu == cpu {
            region_ref = Some(region);
            break;
        }
    }

    let region_ref = region_ref.unwrap();
    let offset = region_ref.used.next_multiple_of(alignment);
    assert!(region_ref.used + len <= 4096);
    region_ref.used += len;

    log::info!("end alloc");
    offset
}
