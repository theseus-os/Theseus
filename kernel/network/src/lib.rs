#![no_std]
#![feature(alloc)]

#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate smoltcp;
extern crate e1000;
extern crate tsc;
extern crate spin;

#[macro_use] extern crate lazy_static;

extern crate irq_safety;

pub mod server;
pub mod nw_server;
//pub mod server_once;

