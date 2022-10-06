//! Offers the `pause` instruction, which rustc used to use for 
//! `spin_loop_hint()` and `spin_loop()` before March 15th, 2019. 
//! 
//! For some reason, their current implementation of those functions
//! emits the `_mm_pause()` intrinsic *only when* "sse2" is enabled,
//! instead of always emitting it regardless of configuration.
//! The lack of `pause` harms Theseus's performance within the QEMU emulator.
//! No effect is noticeable on KVM or on real hardware.

#![no_std]

/// A wrapper around the `pause` x86 ASM function. 
/// On non-x86_64 architectures, this is a no-op (empty function).
#[inline(always)]
pub fn spin_loop_hint() {
    // core::hint::spin_loop();
    #[cfg(target_arch = "x86_64")]
    unsafe { core::arch::asm!("pause", options(nomem, nostack)); }
}
