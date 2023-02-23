//! CPU idle management.
//!
//! Currently, this crate is incomplete. In future it will provide an idle loop
//! which dynamically selects a sleep state for the CPU based on a set of
//! heuristics.

#![no_std]

mod arch;

pub use arch::*;
