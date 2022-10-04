use crate::Instant;
use core::time::Duration;

type _AtomicFn = crossbeam::atomic::AtomicCell<fn()>;
static_assertions::const_assert!(_AtomicFn::is_lock_free());
static_assertions::const_assert_eq!(core::mem::size_of::<_AtomicFn>(), 8);

pub(crate) fn early_sleep(_: Duration) {
    panic!("called early_sleep dummy function");
}

pub(crate) fn monotonic_now() -> Instant {
    panic!("called monotonic_now dummy function");
}

pub(crate) fn instant_to_duration(_: Instant) -> Duration {
    panic!("called instant_to_duration dummy function");
}

pub(crate) fn duration_to_instant(_: Duration) -> Instant {
    panic!("called duration_to_instant dummy function");
}

pub(crate) fn realtime_now() -> Duration {
    panic!("called realtime_now dummy function");
}
