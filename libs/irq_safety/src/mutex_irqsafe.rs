use core::marker::Sync;
use core::ops::{Drop, Deref, DerefMut};
use core::fmt;
use core::option::Option::{self, None, Some};
use core::default::Default;
use core::mem::ManuallyDrop;

use spin::{Mutex, MutexGuard};
use held_interrupts::{HeldInterrupts, hold_interrupts};

/// This type provides interrupt-safe MUTual EXclusion based on [spin::Mutex].
///
/// # Description
///
/// This structure behaves a lot like a normal MutexIrqSafe. There are some differences:
///
/// - It may be used outside the runtime.
///   - A normal MutexIrqSafe will fail when used without the runtime, this will just lock
///   - When the runtime is present, it will call the deschedule function when appropriate
/// - No lock poisoning. When a fail occurs when the lock is held, no guarantees are made
///
/// When calling rust functions from bare threads, such as C `pthread`s, this lock will be very
/// helpful. In other cases however, you are encouraged to use the locks from the standard
/// library.
///
/// # Simple examples
///
/// ```
/// use spin;
/// let spin_mutex = spin::MutexIrqSafe::new(0);
///
/// // Modify the data
/// {
///     let mut data = spin_mutex.lock();
///     *data = 2;
/// }
///
/// // Read the data
/// let answer =
/// {
///     let data = spin_mutex.lock();
///     *data
/// };
///
/// assert_eq!(answer, 2);
/// ```
///
/// # Thread-safety example
///
/// ```
/// use spin;
/// use std::sync::{Arc, Barrier};
///
/// let numthreads = 1000;
/// let spin_mutex = Arc::new(spin::MutexIrqSafe::new(0));
///
/// // We use a barrier to ensure the readout happens after all writing
/// let barrier = Arc::new(Barrier::new(numthreads + 1));
///
/// for _ in (0..numthreads)
/// {
///     let my_barrier = barrier.clone();
///     let my_lock = spin_mutex.clone();
///     std::thread::spawn(move||
///     {
///         let mut guard = my_lock.lock();
///         *guard += 1;
///
///         // Release the lock to prevent a deadlock
///         drop(guard);
///         my_barrier.wait();
///     });
/// }
///
/// barrier.wait();
///
/// let answer = { *spin_mutex.lock() };
/// assert_eq!(answer, numthreads);
/// ```
pub struct MutexIrqSafe<T: ?Sized>
{
    lock: Mutex<T>,
}

/// A guard to which the protected data can be accessed
///
/// When the guard falls out of scope it will release the lock.
pub struct MutexGuardIrqSafe<'a, T: ?Sized + 'a>
{
    held_irq: ManuallyDrop<HeldInterrupts>,
    guard: ManuallyDrop<MutexGuard<'a, T>>, 
}

// Same unsafe impls as `std::sync::MutexIrqSafe`
unsafe impl<T: ?Sized + Send> Sync for MutexIrqSafe<T> {}
unsafe impl<T: ?Sized + Send> Send for MutexIrqSafe<T> {}

impl<T> MutexIrqSafe<T>
{
    /// Creates a new spinlock wrapping the supplied data.
    ///
    /// May be used statically:
    ///
    /// ```
    /// #![feature(const_fn)]
    /// use spin;
    ///
    /// static MutexIrqSafe: spin::MutexIrqSafe<()> = spin::MutexIrqSafe::new(());
    ///
    /// fn demo() {
    ///     let lock = MutexIrqSafe.lock();
    ///     // do something with lock
    ///     drop(lock);
    /// }
    /// ```
    #[cfg(feature = "const_fn")]
    pub const fn new(user_data: T) -> MutexIrqSafe<T>
    {
        MutexIrqSafe
        {
            lock: Mutex::new(user_data),
        }
    }

    /// Creates a new spinlock wrapping the supplied data.
    ///
    /// If you want to use it statically, you can use the `const_fn` feature.
    ///
    /// ```
    /// use spin;
    ///
    /// fn demo() {
    ///     let MutexIrqSafe = spin::MutexIrqSafe::new(());
    ///     let lock = MutexIrqSafe.lock();
    ///     // do something with lock
    ///     drop(lock);
    /// }
    /// ```
    #[cfg(not(feature = "const_fn"))]
    pub fn new(user_data: T) -> MutexIrqSafe<T>
    {
        MutexIrqSafe
        {
            lock: Mutex::new(user_data),
        }
    }

    /// Consumes this MutexIrqSafe, returning the underlying data.
    pub fn into_inner(self) -> T {
        self.lock.into_inner()
    }
}

impl<T: ?Sized> MutexIrqSafe<T>
{
    // fn obtain_lock(&self)
    // {
    //     while self.lock.compare_and_swap(false, true, Ordering::Acquire) != false
    //     {
    //         // Wait until the lock looks unlocked before retrying
    //         while self.lock.load(Ordering::Relaxed)
    //         {
    //             cpu_relax();
    //         }
    //     }
    // }

    /// Locks the spinlock and returns a guard.
    ///
    /// The returned value may be dereferenced for data access
    /// and the lock will be dropped when the guard falls out of scope.
    ///
    /// ```
    /// let mylock = spin::MutexIrqSafe::new(0);
    /// {
    ///     let mut data = mylock.lock();
    ///     // The lock is now locked and the data can be accessed
    ///     *data += 1;
    ///     // The lock is implicitly dropped
    /// }
    ///
    /// ```
    pub fn lock(&self) -> MutexGuardIrqSafe<T>
    {
        MutexGuardIrqSafe
        {
            held_irq: ManuallyDrop::new(hold_interrupts()),
            guard: ManuallyDrop::new(self.lock.lock())
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
        self.lock.force_unlock()
    }

    /// Tries to lock the MutexIrqSafe. If it is already locked, it will return None. Otherwise it returns
    /// a guard within Some.
    pub fn try_lock(&self) -> Option<MutexGuardIrqSafe<T>>
    {
        match self.lock.try_lock() {
            None => None,
            success => {
                Some(
                    MutexGuardIrqSafe {
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
        match self.lock.try_lock()
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

impl<'a, T: ?Sized> Deref for MutexGuardIrqSafe<'a, T>
{
    type Target = T;

    fn deref<'b>(&'b self) -> &'b T { 
        & *(self.guard) 
    }
}

impl<'a, T: ?Sized> DerefMut for MutexGuardIrqSafe<'a, T>
{
    fn deref_mut<'b>(&'b mut self) -> &'b mut T { 
        &mut *(self.guard)
    }
}


// NOTE: we need explicit calls to .drop() to ensure that HeldInterrupts are not released 
//       until the inner lock is also released.
impl<'a, T: ?Sized> Drop for MutexGuardIrqSafe<'a, T>
{
    /// The dropping of the MutexGuardIrqSafe will release the lock it was created from.
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.guard);
            ManuallyDrop::drop(&mut self.held_irq);
        }
    }
}