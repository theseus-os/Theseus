mod block;
mod irq;
mod preemption;
mod spin;

pub use self::preemption::PreemptionSafe;
pub use self::spin::Spin;
pub use irq::IrqSafe;

pub trait DeadlockPrevention: private::Sealed {
    type Guard: private::Sealed;

    fn enter() -> Self::Guard;
}

pub trait MutexFlavour: private::Sealed {
    type LockData: LockData;

    type GuardData<'a>: GuardData<LockData = Self::LockData>;

    fn slow_path<F, G>(data: &Self::LockData, try_lock: F) -> G
    where
        F: Fn() -> Option<G>;
}

pub trait LockData: private::Sealed {
    fn new() -> Self;
}

pub trait GuardData: private::Sealed {
    type LockData: LockData;

    fn new(lock_data: &Self::LockData) -> Self;
}

impl<T> MutexFlavour for T
where
    T: DeadlockPrevention,
{
    type LockData = ();

    type GuardData<'a> = GuardWrapper<T>;

    fn slow_path<F, G>(_: &Self::LockData, try_lock: F) -> G
    where
        F: Fn() -> Option<G>,
    {
        loop {
            if let Some(guard) = try_lock() {
                return guard;
            }
            core::hint::spin_loop();
        }
    }
}

#[doc(hidden)]
pub struct GuardWrapper<T>
where
    T: DeadlockPrevention,
{
    _inner: T::Guard,
}

impl<T> private::Sealed for GuardWrapper<T> where T: DeadlockPrevention {}

impl<T> GuardData for GuardWrapper<T>
where
    T: DeadlockPrevention,
{
    type LockData = ();

    fn new(_: &Self::LockData) -> Self {
        Self { _inner: T::enter() }
    }
}

impl private::Sealed for () {}

impl LockData for () {
    fn new() -> Self {
        ()
    }
}

mod private {
    pub trait Sealed {}
}
