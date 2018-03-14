//! The frame buffer for display on the screen


#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]

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
        self.buffer().chars[32160].write(0x20);
        self.buffer().chars[32161].write(0x20);

        self.buffer().chars[32162].write(0x20);
        self.buffer().chars[32163].write(0x20);
        self.buffer().chars[32164].write(0x20);
        self.buffer().chars[32165].write(0x20);
        self.buffer().chars[32166].write(0x20);
        self.buffer().chars[32167].write(0x20);
        self.buffer().chars[32168].write(0x20);
        self.buffer().chars[32169].write(0x20);
        self.buffer().chars[32170].write(0x20);
        self.buffer().chars[32171].write(0x20);
        self.buffer().chars[32172].write(0x20);
        self.buffer().chars[32173].write(0x20);
        

        let a =&(&self.buffer()).chars[32160].read();
        //trace!("Symbol {:#?}", a);

    }


    fn buffer(&mut self) -> &mut Buffer {
        unsafe { self.buffer.as_mut() }
    }  
}


struct Buffer {
    chars: [Volatile<u8>; 320*200]
}


