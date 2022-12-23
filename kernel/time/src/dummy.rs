use crate::Instant;
use core::time::Duration;
use log::error;

type _AtomicFn = crossbeam_utils::atomic::AtomicCell<fn()>;
static_assertions::const_assert!(_AtomicFn::is_lock_free());
static_assertions::const_assert_eq!(core::mem::size_of::<_AtomicFn>(), 8);

pub(crate) fn early_sleep(_: Duration) {
    error!("called early_sleep dummy function");
}

pub(crate) fn monotonic_now() -> Instant {
    error!("called monotonic_now dummy function");
    Instant::ZERO
}

pub(crate) fn wall_time_now() -> Duration {
    error!("called wall_time_now dummy function");
    Duration::ZERO
}
