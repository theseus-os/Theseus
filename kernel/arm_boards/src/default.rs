//! Default board config file.
//!
//! This will result in a compile-time error if used in an aarch64 build.

#[cfg(target_arch = "aarch64")]
compile_error!("Please select a board config feature in the arm_boards crate");
