//! A library for defining CPU-local variables.
//!
//! See [`cpu_local`] for more details.

#![no_std]

extern crate alloc;

pub use cls_macros::cpu_local;

// TODO: Support multiple integer sizes?
/// Trait for types that can be stored in a single register.
pub trait Raw {
    /// Returns a `u64` representation of the struct.
    fn into_raw(self) -> u64;

    /// Recreates the struct from the `u64` representation.
    ///
    /// # Safety
    ///
    /// The raw representation must have been previously returned from a call to
    /// [`<Self as RawRepresentation>::into_raw`]. Furthermore, `from_raw` must
    /// only be called once per `u64` returned from [`<Self as
    /// RawRepresentation>::into_raw`].
    ///
    /// [`<Self as RawRepresentation>::into_raw`]: RawRepresentation::into_raw
    unsafe fn from_raw(raw: u64) -> Self;
}

impl Raw for u8 {
    fn into_raw(self) -> u64 {
        u64::from(self)
    }

    unsafe fn from_raw(raw: u64) -> Self {
        // Guaranteed to fit since it was created by `into_raw`.
        Self::try_from(raw).unwrap()
    }
}

impl Raw for u16 {
    fn into_raw(self) -> u64 {
        u64::from(self)
    }

    unsafe fn from_raw(raw: u64) -> Self {
        // Guaranteed to fit since it was created by `into_raw`.
        Self::try_from(raw).unwrap()
    }
}

impl Raw for u32 {
    fn into_raw(self) -> u64 {
        u64::from(self)
    }

    unsafe fn from_raw(raw: u64) -> Self {
        // Guaranteed to fit since it was created by `into_raw`.
        Self::try_from(raw).unwrap()
    }
}

impl Raw for u64 {
    fn into_raw(self) -> u64 {
        self
    }

    unsafe fn from_raw(raw: u64) -> Self {
        raw
    }
}

use alloc::vec::Vec;

use sync_spin::SpinMutex;

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

    // TODO: Aarch64
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
