//! Media access protocols.
//!
//! These protocols can be used to enumerate and access various media devices.
//! They provide both **high-level abstractions** such as **files and partitions**,
//! and **low-level access** such as an **block I/O** or **raw ATA** access protocol.

pub mod file;

pub mod fs;
