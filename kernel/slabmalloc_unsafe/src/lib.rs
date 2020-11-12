// Copyright (c) 2015 Gerd Zellweger. See the README.md
// file at the top-level directory of this distribution.
//
// A slab allocator adapted from the slabmalloc crate.
// The original can be found at https://crates.io/crates/slabmalloc


//! A slab allocator implementation for objects less than 8KiB.
//! This allocator does not use any of Theseus's memory management abstractions, and 
//! works with pointers to the start of allocable pages.
//!
//! # Overview
//!
//! The organization is as follows:
//!
//!  * A `ZoneAllocator` manages many `SCAllocator` and can
//!    satisfy requests for different allocation sizes.
//!  * A `SCAllocator` allocates objects of exactly one size.
//!    It stores the objects and meta-data in one or multiple `AllocablePage` objects.
//!  * A trait `AllocablePage` that defines the page-type from which we allocate objects.
//!
//! Lastly, it provides a default `AllocablePage` implementations `ObjectPage8k` that is 8 KiB in size 
//! and contains allocated objects and associated meta-data

#![cfg_attr(feature = "unstable", feature(const_fn))]
#![feature(const_mut_refs)]
#![no_std]

// #[macro_use] extern crate log;

mod pages;
mod sc;
mod zone;

pub use pages::*;
pub use sc::*;
pub use zone::*;

use core::alloc::Layout;
use core::fmt;
use core::mem;
use core::ptr::{self, NonNull};


#[cfg(target_arch = "x86_64")]
const CACHE_LINE_SIZE: usize = 64;

#[cfg(target_arch = "x86_64")]
type VAddr = usize;
