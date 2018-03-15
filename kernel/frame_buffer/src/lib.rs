//! The frame buffer for display on the screen


#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(unique)]
#![feature(asm)]

extern crate spin;

extern crate volatile;
extern crate alloc;
extern crate serial_port;
extern crate kernel_config;
extern crate state_store;
#[macro_use] extern crate log;

use core::ptr::Unique;
use core::fmt;
use spin::Mutex;
use volatile::Volatile;
use alloc::string::String;
use kernel_config::memory::KERNEL_OFFSET;
use state_store::{SSCached, get_state, insert_state};

const FRAME_BUFFER_ADDR: usize = 0xa0000;
const FRAME_BUFFER_WIDTH:usize = 800*3;
const FRAME_BUFFER_HEIGHT:usize = 600;


pub static FRAME_DRAWER: Mutex<Drawer> = Mutex::new(Drawer {
    buffer: unsafe { Unique::new_unchecked((FRAME_BUFFER_ADDR + KERNEL_OFFSET) as *mut _) },
});



#[macro_export]
macro_rules! draw_pixel {
    ($($arg:tt)*) => ({
        $crate::draw_pixel();
    });
}


#[doc(hidden)]
pub fn draw_pixel() {
   unsafe{ FRAME_DRAWER.force_unlock();}
    FRAME_DRAWER.lock().draw_pixel();
}

pub struct Drawer {
    buffer: Unique<Buffer>,
}
impl Drawer {
    pub fn draw_pixel(&mut self) {
        for i in 0..300 {
            let a = 85;
            self.buffer().chars[100000 + i*3].write(0x66);
            self.buffer().chars[100000 + i*3+1].write(0xab);
            self.buffer().chars[100000 + i*3+2].write(0x20);

        }

        unsafe{
                asm!("mov al, 0x13": : : : "intel", "volatile");
                asm!("mov ah, 0x00": : : : "intel", "volatile");
        } 
        

    }


    fn buffer(&mut self) -> &mut Buffer {
        unsafe { self.buffer.as_mut() }
    }  
}


struct Buffer {
    chars: [Volatile<u8>; 640*3*480]
}


