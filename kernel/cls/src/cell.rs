use core::{cell::UnsafeCell, marker::PhantomData, mem};
use sync::DeadlockPrevention;

pub struct CpuLocalCell<T, P>
where
    P: DeadlockPrevention,
{
    value: UnsafeCell<T>,
    prevention: PhantomData<*const P>,
}

impl<T, P> CpuLocalCell<T, P>
where
    P: DeadlockPrevention,
{
    /// Creates a new cell containing the given value.
    ///
    /// # Safety
    ///
    /// The method of deadlock prevention must be correct for the data accesses.
    #[inline]
    pub const unsafe fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            prevention: PhantomData,
        }
    }

    /// Sets the contained value.
    #[inline]
    pub fn set(&self, value: T) {
        let guard = P::enter();
        self.replace(value);
        drop(guard);
    }

    /// Replaces the contained value, returning the old value.
    #[inline]
    pub fn replace(&self, value: T) -> T {
        let guard = P::enter();
        let old_value = mem::replace(unsafe { &mut *self.value.get() }, value);
        drop(guard);
        old_value
    }
}

impl<T, P> CpuLocalCell<T, P>
where
    T: Copy,
    P: DeadlockPrevention,
{
    /// Returns a copy of the contained value.
    pub fn get(&self) -> T {
        let guard = P::enter();
        let value = unsafe { *self.value.get() };
        drop(guard);
        value
    }

    /// Updates the contained value using a function and returns the new value.
    pub fn update<F>(&self, f: F) -> T
    where
        F: FnOnce(T) -> T,
    {
        let old = self.get();
        let new = f(old);
        self.set(new);
        new
    }
}

impl<T, P> !Send for CpuLocalCell<T, P> where P: DeadlockPrevention {}

// TODO: Should T: Sync? I don't think so, because we aren't actually sharing
// the T across threads.
// SAFETY
unsafe impl<T, P> Sync for CpuLocalCell<T, P> where P: DeadlockPrevention {}
