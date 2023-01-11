// TODO: Use features.

cfg_if::cfg_if! {
    if #[cfg(priority_scheduler)] {
        mod priority;
        pub use self::priority::*;
    } else if #[cfg(realtime_scheduler)] {
        mod realtime;
        pub use self::realtime::*;
    } else {
        mod round_robin;
        pub use self::round_robin::*;
    }
}
