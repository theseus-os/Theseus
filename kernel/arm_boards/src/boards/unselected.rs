//! Invalid board config file, selected by default if you didn't select one.
//!
//! This will result in a compile-time error if used in an aarch64 build.

compile_error!("Please select a board config feature in the arm_boards crate");
