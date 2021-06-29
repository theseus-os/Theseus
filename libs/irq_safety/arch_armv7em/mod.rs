use core::fmt;
use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};
use spin::{Mutex, MutexGuard};
use cortex_m::{interrupt, register};
use owning_ref::{OwningRef, OwningRefMut};
use stable_deref_trait::StableDeref;

/// This type provides interrupt-safe MUTual EXclusion based on [spin::Mutex].
///
/// # Description
///
/// This structure behaves a lot like a normal Mutex, but will disable the interrupt
/// for the period of being locked.
pub struct MutexIrqSafe<T: ?Sized> {
    mtx: Mutex<T>
}

/// A guard to which the protected data can be accessed.
///
/// When the guard falls out of scope it will release the lock, and will re-enable the
/// interrupt if at the time of locking the interrupt was enabled.
pub struct MutexIrqSafeGuard<'a, T: ?Sized + 'a>
{
    held_irq: ManuallyDrop<HeldInterrupts>,
    guard: ManuallyDrop<MutexGuard<'a, T>>, 
}

unsafe impl<T: ?Sized + Send> Sync for MutexIrqSafe<T> {}
unsafe impl<T: ?Sized + Send> Send for MutexIrqSafe<T> {}

impl<T> MutexIrqSafe<T> {
    /// Creates a new spinlock wrapping the supplied data.
    pub fn new(user_data: T) -> MutexIrqSafe<T>
    {
        MutexIrqSafe
        {
            mtx: Mutex::new(user_data)
        }
    }

    /// Consumes this MutexIrqSafe, returning the underlying data.
    pub fn into_inner(self) -> T {
        self.mtx.into_inner()
    }
}

impl<T: ?Sized> MutexIrqSafe<T> {
    /// Locks the spinlock, disables the interrupt, and returns a guard.
    ///
    /// The returned value may be dereferenced for data access
    /// and the lock will be dropped when the guard falls out of scope.
    /// The interrupt will be re-enabled when the guard falls out of scope if
    /// at the time of locking the interrupt is enabled.
    pub fn lock(&self) -> MutexIrqSafeGuard<T>
    {
        MutexIrqSafeGuard
        {
            held_irq: ManuallyDrop::new(hold_interrupts()),
            guard: ManuallyDrop::new(self.mtx.lock())
        }
    }

    /// Force unlock the spinlock.
    ///
    /// This is *extremely* unsafe if the lock is not held by the current
    /// thread. However, this can be useful in some instances for exposing the
    /// lock to FFI that doesn't know how to deal with RAII.
    ///
    /// If the lock isn't held, this is a no-op.
    pub unsafe fn force_unlock(&self) {
        self.mtx.force_unlock()
    }

    /// Tries to lock the MutexIrqSafe. If it is already locked, it will return None.
    /// Otherwise it returns a guard within Some.
    pub fn try_lock(&self) -> Option<MutexIrqSafeGuard<T>> {
        match self.mtx.try_lock() {
            None => None,
            success => {
                Some(
                    MutexIrqSafeGuard {
                        held_irq: ManuallyDrop::new(hold_interrupts()),
                        guard: ManuallyDrop::new(success.unwrap()),
                    }
                )
            }
        }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for MutexIrqSafe<T>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        match self.mtx.try_lock()
        {
            Some(guard) => write!(f, "MutexIrqSafe {{ data: {:?} }}", &*guard),
            None => write!(f, "MutexIrqSafe {{ <locked> }}"),
        }
    }
}

impl<T: ?Sized + Default> Default for MutexIrqSafe<T> {
    fn default() -> MutexIrqSafe<T> {
        MutexIrqSafe::new(Default::default())
    }
}

impl<'a, T: ?Sized> Deref for MutexIrqSafeGuard<'a, T> {
    type Target = T;

    fn deref<'b>(&'b self) -> &'b T { 
        & *(self.guard) 
    }
}

impl<'a, T: ?Sized> DerefMut for MutexIrqSafeGuard<'a, T> {
    fn deref_mut<'b>(&'b mut self) -> &'b mut T { 
        &mut *(self.guard)
    }
}


// NOTE: we need explicit calls to .drop() to ensure that HeldInterrupts are not released 
//       until the inner lock is also released.
impl<'a, T: ?Sized> Drop for MutexIrqSafeGuard<'a, T> {
    /// The dropping of the MutexIrqSafeGuard will release the lock it was created from.
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.guard);
            ManuallyDrop::drop(&mut self.held_irq);
        }
    }
}

// Implement the StableDeref trait for MutexIrqSafe guards, just like it's implemented for Mutex guards
unsafe impl<'a, T: ?Sized> StableDeref for MutexIrqSafeGuard<'a, T> {}

/// Typedef of a owning reference that uses a `MutexIrqSafeGuard` as the owner.
pub type MutexIrqSafeGuardRef<'a, T, U = T> = OwningRef<MutexIrqSafeGuard<'a, T>, U>;
/// Typedef of a mutable owning reference that uses a `MutexIrqSafeGuard` as the owner.
pub type MutexIrqSafeGuardRefMut<'a, T, U = T> = OwningRefMut<MutexIrqSafeGuard<'a, T>, U>;

/// A handle for frozen interrupts
#[derive(Default)]
pub struct HeldInterrupts(bool);

/// Prevent interrupts from firing until the return value is dropped (goes out of scope). 
/// After it is dropped, the interrupts are returned to their prior state, not blindly re-enabled. 
pub fn hold_interrupts() -> HeldInterrupts {
    let enabled = interrupts_enabled();
	let retval = HeldInterrupts(enabled);
    disable_interrupts();
    retval
}


impl core::ops::Drop for HeldInterrupts {
	fn drop(&mut self)
	{
		if self.0 {
			unsafe { enable_interrupts(); }
		}
	}
}

#[inline(always)]
pub unsafe fn enable_interrupts() {
    interrupt::enable()
}

#[inline(always)]
pub fn disable_interrupts() {
    interrupt::disable()
}

#[inline(always)]
pub fn interrupts_enabled() -> bool {
    register::primask::read().is_active()
}
