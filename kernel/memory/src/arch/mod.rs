#[cfg(any(target_arch = "x86_64"))]
pub mod x86_64;
#[cfg(any(target_arch = "aarch64"))]
pub mod aarch64;

#[cfg(any(target_arch = "x86_64"))]
pub use self::x86_64::{EntryFlags, KERNEL_OFFSET_BITS_START, KERNEL_OFFSET_PREFIX, set_new_p4, get_current_p4, flush, tlb};
#[cfg(any(target_arch = "aarch64"))]
pub use self::aarch64::{EntryFlags, KERNEL_OFFSET_BITS_START, KERNEL_OFFSET_PREFIX, set_new_p4, get_current_p4, flush, tlb};
