#![no_std]
#![feature(stmt_expr_attributes)]

#[cfg(test)]
#[macro_use] extern crate std;

extern crate alloc;

/// A basic atomic linked list. 
pub mod atomic_linked_list;

/// A basic map structure which is backed by an atomic linked list. 
pub mod atomic_map;
