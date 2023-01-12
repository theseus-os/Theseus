// TODO: Use features.

cfg_if::cfg_if! {
    if #[cfg(priority_scheduler)] {
        mod priority;
        pub(crate) use self::priority::*;
    } else if #[cfg(realtime_scheduler)] {
        mod realtime;
        pub(crate) use self::realtime::*;
    } else {
        mod round_robin;
        pub(crate) use self::round_robin::*;
    }
}
