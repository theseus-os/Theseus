//! This crate exists solely to hold a centralized list of features 
//! used across all Theseus crates for conditional compilation and configuration.
//!  
//! It has no code, logic, or data.
//! 
//! ## How to specify optional crates/features in Theseus
//! 
//! To make a crate (or folder of crates) optional, do the following:
//! 1. Add it to the set of `exclude`s in the top-level Theseus `Cargo.toml` file.
//! 2. Add it as an optional dependency in this crate's `Cargo.toml` file, ensuring `optional = true`.
//! 3. (Optional) Add it as a feature below, in order to give it a specific name.
//!    * Technically this isn't required, but it can offer a clearer/different name.
//!    * You can optionally add it to the `default` set of features,
//!      or create a new group of features that includes it, 
//!      or even add it to an existing set of features.
//!    * If you do so, add that new feature or feature group to the `everything` feature.
//!
//! 
//! ## How to customize what is included in a Theseus build
//! 
//! When building Theseus using `make`, you can choose which of the features 
//! specified in this crate's `Cargo.toml` file are enabled.
//! Simply set the `FEATURES` environment variable, which has the default value of `--workspace`.
//! 
//! The `FEATURES` variable is passed directly into `cargo build`, meaning that is must
//! follow the formatting expected by `cargo build.
//! > Run `cargo build --help` to see more details about what arguments cargo expects.
//! 
//! ```sh
//! # Build the bare minimum `default-members` of the Theseus workspace, 
//! # excluding those specified by the `default` feature in this crate's `Cargo.toml`.
//! make FEATURES=--no-default-features
//! 
//! # Build the bare minimum `default-members` of the Theseus workspace, plus all optional crates.
//! make FEATURES="--features everything"
//! 
//! # Build the standard Theseus workspace plus all optional crates. 
//! # This is what `make full` or `make all` does.
//! make FEATURES="--workspace --features theseus_features/everything"
//! 
//! # Build the bare minimum `default-members` of the Theseus workspace, plus the `ps` crate.
//! make FEATURES="--features ps"
//! 
//! # Build the standard Theseus workspace plus the crates needed for the below `wasmtime` feature
//! # and the `test_panic` crate.
//! make FEATURES="--workspace --features wasmtime --features test_panic" 
//! ```


#![no_std]

// Intentionally left blank.
