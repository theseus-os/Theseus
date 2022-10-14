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

pub(crate) fn instant_to_duration(_: Instant) -> Duration {
    error!("called instant_to_duration dummy function");
    Duration::ZERO
}

pub(crate) fn duration_to_instant(_: Duration) -> Instant {
    error!("called duration_to_instant dummy function");
    Instant::ZERO
}

pub(crate) fn wall_time_now() -> Duration {
    error!("called wall_time_now dummy function");
    Duration::ZERO
}
