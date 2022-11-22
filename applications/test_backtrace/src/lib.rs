//! Testing backtrace functionality of the `backtrace` crate ported to Theseus.
//! 
//! Note: we use the `black_box()` function to forcibly avoid inlining.
//! 

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate app_io;
extern crate task;

extern crate backtrace;


use alloc::vec::Vec;
use alloc::string::String;


pub fn main(_args: Vec<String>) -> isize {
    info!("test_backtrace::main(): at top");
    core::hint::black_box(bar());
    0
}


#[inline(never)]
fn bar() {
    core::hint::black_box(foo());
}

#[inline(never)]
fn foo() {
    core::hint::black_box(baz());
}

#[inline(never)]
fn baz() {
    core::hint::black_box(print_backtrace());
}

#[inline(never)]
pub fn print_backtrace() {
    println!("\n======================================================");
    println!("Testing simple backtrace:");
    backtrace::trace(|frame| {
        println!("Frame: {:X?}", frame);
        true
    });

    println!("\n======================================================");
    println!("Testing resolved backtrace:");
    backtrace::trace(|frame| {
        println!("Frame: {:X?}", frame);
        backtrace::resolve_frame(frame, |symbol| println!("    Symbol: {:X?}", symbol));
        true
    });

    println!("\n======================================================");
    println!("Testing captured unresolved backtrace:");
    let mut bt = backtrace::Backtrace::new_unresolved();
    println!("{:X?}", bt);
    println!("Frames: {:X?}", bt.frames());

    println!("\n======================================================");
    println!("Resolving unresolved captured backtrace:");
    bt.resolve();
    println!("{:X?}", bt);
    println!("Frames: {:X?}", bt.frames());

    println!("\n======================================================");
    println!("Starting new resolved backtrace capture:");
    println!("{:X?}", backtrace::Backtrace::new());
}
