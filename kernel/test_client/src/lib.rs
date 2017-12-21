#![no_std]

extern crate test_server;

use test_server::*;

pub fn client_func() -> (u8, u64) {
	let arg1 = generic_fn(10);
	let arg2 = generic_fn(20);
	server_func(arg1, arg2)
}