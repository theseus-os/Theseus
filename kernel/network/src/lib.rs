#![no_std]
#![feature(alloc)]

#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate smoltcp;
extern crate e1000;
extern crate spin;
extern crate dfqueue;
extern crate acpi;


pub mod server;
pub mod e1000_to_smoltcp_interface;
pub mod config;


