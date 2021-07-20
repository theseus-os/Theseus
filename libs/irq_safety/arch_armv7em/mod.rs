use core::fmt;
use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};
use spin::{Mutex, MutexGuard};
use spin::{RwLock, RwLockReadGuard, RwLockWriteGuard};
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

pub struct RwLockIrqSafe<T: ?Sized> {
    rwlock: RwLock<T>,
}


/// A guard to which the protected data can be read
///
/// When the guard falls out of scope it will decrement the read count,
/// potentially releasing the lock and potentially re-enabling interrupts.
pub struct RwLockIrqSafeReadGuard<'a, T: 'a + ?Sized>
{
    held_irq: ManuallyDrop<HeldInterrupts>,
    guard: ManuallyDrop<RwLockReadGuard<'a, T>>,
}

/// A guard to which the protected data can be written
///
/// When the guard falls out of scope it will release the lock and potentially re-enable interrupts.
pub struct RwLockIrqSafeWriteGuard<'a, T: 'a + ?Sized>
{
    held_irq: ManuallyDrop<HeldInterrupts>,
    guard: ManuallyDrop<RwLockWriteGuard<'a, T>>,
}

// Same unsafe impls as `std::sync::RwLock`
unsafe impl<T: ?Sized + Send + Sync> Send for RwLockIrqSafe<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLockIrqSafe<T> {}


impl<T> RwLockIrqSafe<T>
{
    /// Creates a new spinlock wrapping the supplied data.
    ///
    /// May be used statically:
    ///
    /// ```
    /// #![feature(const_fn)]
    ///
    /// static RW_LOCK_IRQ_SAFE: RwLockIrqSafe<()> = RwLockIrqSafe::new(());
    ///
    /// fn demo() {
    ///     let lock = RW_LOCK_IRQ_SAFE.read();
    ///     // do something with lock
    ///     drop(lock);
    /// }
    /// ```
    #[inline]
    #[cfg(feature = "const_fn")]
    pub const fn new(user_data: T) -> RwLockIrqSafe<T>
    {
        RwLockIrqSafe
        {
            rwlock: RwLock::new(user_data),
        }
    }

    /// Creates a new spinlock wrapping the supplied data.
    ///
    /// If you want to use it statically, you can use the `const_fn` feature.
    ///
    /// ```
    ///
    /// fn demo() {
    ///     let rw_lock_irq_safe = RwLockIrqSafe::new(());
    ///     let lock = rw_lock_irq_safe.read();
    ///     // do something with lock
    ///     drop(lock);
    /// }
    /// ```
    #[inline]
    #[cfg(not(feature = "const_fn"))]
    pub fn new(user_data: T) -> RwLockIrqSafe<T>
    {
        RwLockIrqSafe
        {
            rwlock: RwLock::new(user_data),
        }
    }

    /// Consumes this `RwLockIrqSafe`, returning the underlying data.
    pub fn into_inner(self) -> T
    {
        self.rwlock.into_inner()
    }
}

impl<T: ?Sized> RwLockIrqSafe<T>
{
    /// Locks this RwLockIrqSafe with shared read access, blocking the current thread
    /// until it can be acquired.
    ///
    /// The calling thread will be blocked until there are no more writers which
    /// hold the lock. There may be other readers currently inside the lock when
    /// this method returns. This method does not provide any guarantees with
    /// respect to the ordering of whether contentious readers or writers will
    /// acquire the lock first.
    ///
    /// Returns an RAII guard which will release this thread's shared access
    /// once it is dropped, along with restoring interrupts. 
    ///
    /// ```
    /// let mylock = RwLockIrqSafe::new(0);
    /// {
    ///     let mut data = mylock.read();
    ///     // The lock is now locked, interrupts are disabled, and the data can be read
    ///     println!("{}", *data);
    ///     // The lock is dropped and interrupts are restored to their prior state
    /// }
    /// ```
    #[inline]
    pub fn read<'a>(&'a self) -> RwLockIrqSafeReadGuard<'a, T>
    {
        RwLockIrqSafeReadGuard {
            held_irq: ManuallyDrop::new(hold_interrupts()),
            guard:  ManuallyDrop::new(self.rwlock.read()),
        }
    }

    /// Attempt to acquire this lock with shared read access.
    ///
    /// This function will never block and will return immediately if `read`
    /// would otherwise succeed. Returns `Some` of an RAII guard which will
    /// release the shared access of this thread when dropped, or `None` if the
    /// access could not be granted. This method does not provide any
    /// guarantees with respect to the ordering of whether contentious readers
    /// or writers will acquire the lock first.
    ///
    /// ```
    /// let mylock = spin::RwLock::new(0);
    /// {
    ///     match mylock.try_read() {
    ///         Some(data) => {
    ///             // The lock is now locked and the data can be read
    ///             println!("{}", *data);
    ///             // The lock is dropped
    ///         },
    ///         None => (), // no cigar
    ///     };
    /// }
    /// ```
    #[inline]
    pub fn try_read(&self) -> Option<RwLockIrqSafeReadGuard<T>>
    {   
        match self.rwlock.try_read() {
            None => None,
            success => {
                Some(
                    RwLockIrqSafeReadGuard {
                        held_irq: ManuallyDrop::new(hold_interrupts()),
                        guard: ManuallyDrop::new(success.unwrap()),
                    }
                )
            }
        }
    }

    /// Force decrement the reader count.
    ///
    /// This is *extremely* unsafe if there are outstanding `RwLockReadGuard`s
    /// live, or if called more times than `read` has been called, but can be
    /// useful in FFI contexts where the caller doesn't know how to deal with
    /// RAII.
    pub unsafe fn force_read_decrement(&self) {
        self.rwlock.force_read_decrement();
    }

    /// Force unlock exclusive write access.
    ///
    /// This is *extremely* unsafe if there are outstanding `RwLockWriteGuard`s
    /// live, or if called when there are current readers, but can be useful in
    /// FFI contexts where the caller doesn't know how to deal with RAII.
    pub unsafe fn force_write_unlock(&self) {
        self.rwlock.force_write_unlock();
    }

    /// Lock this rwlock with exclusive write access, blocking the current
    /// thread until it can be acquired.
    ///
    /// This function will not return while other writers or other readers
    /// currently have access to the lock.
    ///
    /// Returns an RAII guard which will drop the write access of this rwlock
    /// when dropped.
    ///
    /// ```
    /// let mylock = spin::RwLock::new(0);
    /// {
    ///     let mut data = mylock.write();
    ///     // The lock is now locked and the data can be written
    ///     *data += 1;
    ///     // The lock is dropped
    /// }
    /// ```
    #[inline]
    pub fn write<'a>(&'a self) -> RwLockIrqSafeWriteGuard<'a, T>
    {
        RwLockIrqSafeWriteGuard {
            held_irq: ManuallyDrop::new(hold_interrupts()),
            guard:  ManuallyDrop::new(self.rwlock.write()),
        }
    }

    /// Attempt to lock this rwlock with exclusive write access.
    ///
    /// This function does not ever block, and it will return `None` if a call
    /// to `write` would otherwise block. If successful, an RAII guard is
    /// returned.
    ///
    /// ```
    /// let mylock = spin::RwLock::new(0);
    /// {
    ///     match mylock.try_write() {
    ///         Some(mut data) => {
    ///             // The lock is now locked and the data can be written
    ///             *data += 1;
    ///             // The lock is implicitly dropped
    ///         },
    ///         None => (), // no cigar
    ///     };
    /// }
    /// ```
    #[inline]
    pub fn try_write(&self) -> Option<RwLockIrqSafeWriteGuard<T>>
    {
        match self.rwlock.try_write() {
            None => None,
            success => {
                Some(
                    RwLockIrqSafeWriteGuard {
                        held_irq: ManuallyDrop::new(hold_interrupts()),
                        guard: ManuallyDrop::new(success.unwrap()),
                    }
                )
            }
        }
    }

}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockIrqSafe<T>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        match self.rwlock.try_read()
        {
            Some(guard) => write!(f, "RwLockIrqSafe {{ data: {:?} }}", &*guard),
            None => write!(f, "RwLockIrqSafe {{ <locked> }}"),
        }
    }
}

impl<T: ?Sized + Default> Default for RwLockIrqSafe<T> {
    fn default() -> RwLockIrqSafe<T> {
        RwLockIrqSafe::new(Default::default())
    }
}

impl<'rwlock, T: ?Sized> Deref for RwLockIrqSafeReadGuard<'rwlock, T> {
    type Target = T;

    fn deref(&self) -> &T { 
       & *(self.guard) 
    }
}

impl<'rwlock, T: ?Sized> Deref for RwLockIrqSafeWriteGuard<'rwlock, T> {
    type Target = T;

    fn deref(&self) -> &T { 
        & *(self.guard)
    }
}

impl<'rwlock, T: ?Sized> DerefMut for RwLockIrqSafeWriteGuard<'rwlock, T> {
    fn deref_mut(&mut self) -> &mut T { 
        &mut *(self.guard)
    }
}


// NOTE: we need explicit calls to .drop() to ensure that HeldInterrupts are not released 
//       until the inner lock is also released.
impl<'rwlock, T: ?Sized> Drop for RwLockIrqSafeReadGuard<'rwlock, T> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.guard);
            ManuallyDrop::drop(&mut self.held_irq);
        }
    }
}

impl<'rwlock, T: ?Sized> Drop for RwLockIrqSafeWriteGuard<'rwlock, T> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.guard);
            ManuallyDrop::drop(&mut self.held_irq);
        }
    }
}

// Implement the StableDeref trait for RwLockIrqSafe guards, just like it's implemented for RwLock guards
unsafe impl<'a, T: ?Sized> StableDeref for RwLockIrqSafeReadGuard<'a, T> {}
unsafe impl<'a, T: ?Sized> StableDeref for RwLockIrqSafeWriteGuard<'a, T> {}

/// Typedef of a owning reference that uses a `RwLockIrqSafeReadGuard` as the owner.
pub type RwLockIrqSafeReadGuardRef<'a, T, U = T> = OwningRef<RwLockIrqSafeReadGuard<'a, T>, U>;
/// Typedef of a mutable owning reference that uses a `RwLockIrqSafeWriteGuard` as the owner.
pub type RwLockIrqSafeWriteGuardRefMut<'a, T, U = T> = OwningRefMut<RwLockIrqSafeWriteGuard<'a, T>, U>;


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
