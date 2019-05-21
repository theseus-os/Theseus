//! Offers the 'pause' instruction, which rustc used to use for 
//! `spin_loop_hint()` and `spin_loop()` before March 15th, 2019. 
//! 
//! For some reason, their current implementation of those functions
//! uses the `_mm_pause()` intrinsic, which harms Theseus's performance
//! within the QEMU emulator. No effect is noticeable on KVM or on real hardware.

#![no_std]
#![feature(asm)]

/// A wrapper around the `pause` x86 ASM function. 
/// On non-x86 architectures, this is a no-op (empty function).
pub fn spin_loop_hint() {
    #[cfg(target_arch = "x86_64")]
    unsafe { asm!("pause" ::: "memory" : "volatile"); };
}