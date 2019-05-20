#![no_std]
#![feature(alloc)]

extern crate display;
#[macro_use] extern crate alloc;
#[macro_use] extern crate log;

use display::{VirtualFrameBuffer, Display};
use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    let vf = match VirtualFrameBuffer::new(200,200){
        Ok(vf) => {vf},
        Err(err) => {return 1},
    };

    let mut vf = frame_buffer::map(700, 100, vf).unwrap();
    vf.lock().fill_rectangle(0,0,100,100, 0xff0000);
    match frame_buffer::display(&vf) {
        Ok(_) => {return 0},
        Err(err) => {
            error!("{}", err);
            return -1;
        }
    }
}