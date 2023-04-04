//! Invalid board config file, selected by default if you didn't select one.
//!
//! This will result in a compile-time error if used in an aarch64 build.
//! On x86_64 it's perfectly OK though.

#[cfg(target_arch = "aarch64")]
compile_error!("Please select a board config feature in the arm_boards crate");
