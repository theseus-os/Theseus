//! A library for defining CPU-local variables.
//!
//! See [`cpu_local`] for more details on how to define a CPU-local variable; the information below
//! is not required.
//!
//! # Implementation
//!
//! There are two ways in which a crate can be linked:
//! - Statically, meaning it is linked at compile-time by an external linker.
//! - Dynamically, meaning it is linked at runtime by `mod_mgmt`.
//!
//! Since we have little control over the external linker, we are forced to write CLS code
//! that works with the external linker, despite the linker having no understanding of CLS.
//! This lack of understanding results in significant complexity in calculating the offset
//! of a CLS symbol.
//!
//! Also there is no way to tell whether code was statically or dynamically linked, and so the same
//! CLS code must work with both static and dynamic linking. This results in additional complexity
//! in `mod_mgmt` because it must interface with this generic CLS code.
//!
//! ## x86-64
//!
//! ### Static linking
//!
//! On x86_64, a TLS data image looks like
//!
//! ```text
//!                        fs
//!                         V
//! +-----------------------+--------------+------------------------+
//! | statically linked TLS | TLS self ptr | dynamically linked TLS |
//! +-----------------------+--------------+------------------------+
//! ```
//! where statically linked TLS is accessed using a negative offset from the `fs`
//! register.
//!
//! Now with the way CLS works on Theseus, when we statically link CLS, the
//! linker believes we are prepending the cls section to the tls section like
//! ```
//!                        gs                            fs
//!                         V                             V
//! +-----------------------+-----+-----------------------+
//! | statically linked cls | 000 | statically linked TLS |
//! +-----------------------+-----+-----------------------+
//! ```
//! where `000` are padding bytes to align the start of the statically linked
//! TLS to a page boundary. So the linker will write negative offsets to CLS
//! relocations based on their distance from the end of the statically linked
//! TLS.
//!
//! However, in reality we have a completely separate data image for CLS, and
//! so we need to figure out the negative offset from the `gs` register based on
//! the negative offset from the `fs` register, the CLS size, the TLS size, and
//! the fact that the start of the TLS section is on the next page boundary after
//! the end of the CLS section.
//!
//! ```text
//! from_cls_start
//!    +-----+
//!    |     |
//!    |     |        -{cls}@TPOFF
//!    |     +------------------------------+
//!    |     |                              |
//!    |     |  -offset                     |
//!    |     +----------+                   |
//!    |     |          |                   |
//!    V     V          V                   V
//!    +----------------+-----+-------------+
//!    |      .cls      | 000 | .tls/.tdata |
//!    +----------------+-----+-------------+
//!    ^                ^     ^             ^
//!    |                |     |             |
//!    |                gs    |            fs
//!    |                |     |             |
//!    +----------------+     +-------------+
//!    |    cls_size          |   tls_size
//!    |                      |
//!   a*4kb                  b*4kb
//!    |                      |
//!    +----------------------+
//!     cls_start_to_tls_start
//! ```
//! where `a*4kb` means that the address is a multiple of `4kb` i.e.
//! page-aligned.
//!
//! ### Dynamic linking
//!
//! When a crate is dynamically linked on x86_64, `mod_mgmt` will set `__THESEUS_CLS_SIZE`
//! and `__THESEUS_TLS_SIZE` to `usize::MAX`. Prior to calculating the offset, the CLS
//! access code will check if the variables are set to their sentinel values and if so,
//! it will simply use the provided offset since `mod_mgmt` would have computed it
//! correctly, unlike the external linker.
//!
//! ## aarch64
//!
//! Unlike x86_64, aarch64 doesn't have different branches for statically linked, and
//! dynamically linked variables. This is because on aarch64, there are no negative
//! offset shenanigans. A TLS data image looks like
//! ```
//! fs
//! V
//! +-----------------------+------------------------+
//! | statically linked TLS | dynamically linked TLS |
//! +-----------------------+------------------------+
//! ```
//!
//! Unlike x86_64, the `.cls` section is located after `.tls` in a binary and so the
//! linker thinks the data image looks like
//! ```
//! fs
//! V
//! +-----------------------+-----+-----------------------+------------------------+
//! | statically linked TLS | 000 | statically linked CLS | dynamically linked TLS |
//! +-----------------------+-----+-----------------------+------------------------+
//! ```
//! where `000` are padding bytes to align the start of the statically linked CLS to
//! a page boundary.
//!
//! Hence, CLS symbols have an offset that is incorrect by
//! `tls_size.next_multiple_of(0x1000)`, or 0 if there is not TLS section. So, when
//! loading CLS symbols on aarch64, `mod_mgmt` simply sets `__THESEUS_TLS_SIZE` to 0.

#![no_std]

pub use cls_macros::cpu_local;

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
    pub use preemption;
    
    #[cfg(target_arch = "aarch64")]
    pub use {cortex_a, tock_registers};

    #[cfg(target_arch = "x86_64")]
    pub use x86_64;
}
