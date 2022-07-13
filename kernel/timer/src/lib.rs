#![no_std]

pub use core::time::Duration;

/// A hardware timer.
pub trait Timer {
    fn calibrate() -> Result<(), &'static str>;
    fn value() -> Duration;

    // TODO: configure, period/frequency, and accuracy
}

// pub trait ToggleableTimer: Timer {
//     fn enable();
//     fn disable();
//     fn is_enabled() -> bool;

//     fn is_disabled() -> bool {
//         !Self::is_enabled()
//     }

//     fn toggle() {
//         if Self::is_enabled() {
//             Self::disable();
//         } else {
//             Self::enable();
//         }
//     }
// }
