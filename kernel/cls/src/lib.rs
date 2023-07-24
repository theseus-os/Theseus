#![no_std]

extern crate alloc;

pub use cls_macros::cpu_local;

// pub struct CpuLocalCell<T> {
//     inner: UnsafeCell<T>,
// }

// impl<T> CpuLocalCell<T> {
//     #[doc(hidden)]
//     pub unsafe fn __new(value: T) -> Self {
//         Self {
//             inner: UnsafeCell::new(value),
//         }
//     }

//     pub fn update<F>(f: F) -> R
//     where
//         F: FnOnce(&mut T) -> R,
//     {
//         let guard = preemption::hold_preemption();
//         let inner = unsafe { &mut *self.inner.get() };
//         let ret = f(inner);
//         drop(guard);
//         ret
//     }

//     pub fn update_with<F>(f: F, guard: &PreemptionGuard) -> R
//     where
//         F: FnOnce(&mut T) -> R,
//     {
//         let inner = unsafe { &mut *self.inner.get() };
//         f(inner)
//     }
// }

pub trait RawRepresentation {
    /// # Safety
    unsafe fn into_raw(self) -> u64;
    /// # Safety
    unsafe fn from_raw(raw: u64) -> Self;
}
