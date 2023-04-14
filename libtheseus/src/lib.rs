//! Sample test of a dynamically-linked library atop Theseus kernel crates.

#![no_std]
// #![feature(allocator_api)]
// #![feature(alloc_error_handler)]
// #![feature(lang_items)]
// #![feature(panic_info_message)]

extern crate panic_entry;
extern crate heap;

#[macro_use] extern crate alloc;
// #[macro_use] extern crate terminal_print;
extern crate logger;

use alloc::vec::Vec;
use alloc::string::String;

pub mod my_mod;

pub fn main() {
    libtheseus_hello(vec![String::from("hisss"), String::from("there")]);
    panic!("hello from my main");
}


#[inline(never)]
pub fn libtheseus_hello(_args: Vec<String>) -> isize {
    // println!("Hello from an example dylib main function!");
    logger::write_fmt(format_args!("\n\nHello from libtheseus: args: {:?}", _args)).unwrap();
    0
}



////////////////////////////////////////////////
////// Dummy lang items 
////////////////////////////////////////////////

/*

#[panic_handler] // same as:  #[lang = "panic_impl"]
fn panic_entry_point(_info: &core::panic::PanicInfo) -> ! {
    // println!("panic: {:?}", info);
    loop { }
}

/// This is the callback entry point that gets invoked when the heap allocator runs out of memory.
#[alloc_error_handler]
fn oom(_layout: core::alloc::Layout) -> ! {
    panic!("\n(oom) Out of Heap Memory! requested allocation: {:?}", _layout);
    // loop { }
}

#[lang = "eh_personality"]
#[no_mangle]
extern "C" fn rust_eh_personality() -> ! {
    // println!("BUG: Theseus does not use rust_eh_personality. Why has it been invoked?");
    loop { }
}

#[global_allocator]
pub static GLOBAL_ALLOCATOR: DummyHeap = DummyHeap{};

pub struct DummyHeap;

use alloc::alloc::{GlobalAlloc, Layout};

unsafe impl GlobalAlloc for DummyHeap {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
    }

}


#[no_mangle]
extern "C" fn _Unwind_Resume(_arg: usize) -> ! {
    panic!("_Unwind_Resume invoked");
}

*/
