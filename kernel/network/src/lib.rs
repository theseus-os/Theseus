#![no_std]
#![feature(alloc)]

#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate smoltcp;
extern crate e1000;



pub mod server;
pub mod nw_server;

