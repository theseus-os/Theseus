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

#[macro_use] extern crate log;

use core::ptr::Unique;
use core::fmt;
use spin::Mutex;
use volatile::Volatile;
use alloc::string::String;
use kernel_config::memory::KERNEL_OFFSET;
use alloc::vec::Vec;

const VGA_BUFFER_ADDR: usize = 0xa0000;

//Size of VESA mode 0x4112
pub const FRAME_BUFFER_WIDTH:usize = 640*3;
pub const FRAME_BUFFER_HEIGHT:usize = 480;


pub static FRAME_DRAWER: Mutex<Drawer> = {
    Mutex::new(Drawer {
        start_address:0,
        buffer: unsafe {Unique::new_unchecked((VGA_BUFFER_ADDR) as *mut _) },
    })
};




#[macro_export]
macro_rules! draw_pixel {
    ($x:expr, $y:expr, $color:expr) => ({
        $crate::draw_pixel($x, $y, $color);
    });
}


#[doc(hidden)]
pub fn draw_pixel(x:usize, y:usize, color:usize) {
    unsafe{ FRAME_DRAWER.force_unlock();}
    FRAME_DRAWER.lock().draw_pixel(x, y, color)
}

#[macro_export]
macro_rules! draw_line {
    ($start_x:expr, $start_y:expr, $end_x:expr, $end_y:expr, $color:expr) => ({
        $crate::draw_line($start_x, $start_y, $end_x, $end_y, $color);
    });
}


#[doc(hidden)]
pub fn draw_line(start_x:usize, start_y:usize, end_x:usize, end_y:usize, color:usize) {
    unsafe{ FRAME_DRAWER.force_unlock();}
    FRAME_DRAWER.lock().draw_line(start_x as i32, start_y as i32, end_x as i32, end_y as i32, color)
}

#[macro_export]
macro_rules! draw_square {
    ($start_x:expr, $start_y:expr, $width:expr, $height:expr, $color:expr) => ({
        $crate::draw_square($start_x, $start_y, $width, $height, $color);
    });
}


#[doc(hidden)]
pub fn draw_square(start_x:usize, start_y:usize, width:usize, height:usize, color:usize) {
    unsafe{ FRAME_DRAWER.force_unlock();}
    FRAME_DRAWER.lock().draw_square(start_x, start_y, width, height, color)
}

/*#[macro_export]
 macro_rules! init_frame_buffer {
     ($v_add:expr) => (
         {
             $crate::init_frame_buffer($v_add);
         }
     )
 }


#[doc(hidden)]
pub fn init_frame_buffer(virtual_address:usize) {
    FRAME_DRAWER.lock().init_frame_buffer(virtual_address);
}
*/

pub struct Point {
    pub x: usize,
    pub y: usize,
    pub color: usize,
}



pub struct Drawer {
    start_address: usize,
    buffer: Unique<Buffer> ,
}



impl Drawer {
    pub fn draw_pixel(&mut self, x:usize, y:usize, color:usize){
        if x >= FRAME_BUFFER_WIDTH || y >= FRAME_BUFFER_HEIGHT {
            return
        }
        self.buffer().chars[y][x*3].write((color & 255) as u8);
        self.buffer().chars[y][x*3 + 1].write((color >> 8 & 255) as u8);
        self.buffer().chars[y][x*3 + 2].write((color >> 16 & 255) as u8); 
      
    }

    pub fn draw_points(&mut self, points:Vec<Point>){
        for p in points{
            draw_pixel(p.x, p.y, p.color);
        }
      
    }

    pub fn check_in_range(&mut self, x:usize, y:usize) -> bool {
        x < FRAME_BUFFER_WIDTH && y < FRAME_BUFFER_HEIGHT
    }

    pub fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:usize){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        let mut points = Vec::new();
        if width.abs() > height.abs() {
            let mut y;
            for x in start_x..end_x {
                y = ((x-start_x)*height/width+start_y);
                if(self.check_in_range(x as usize,y as usize)){
                    points.push(Point{x:x as usize, y:y as usize, color:color});
                }
            }
        }
        else {
            let mut x;
            for y in start_y..end_y {
                x = (y-start_y)*width/height+start_x;
                if(self.check_in_range(x as usize,y as usize)){
                    points.push(Point{x:x as usize, y:y as usize, color:color});
                }            
            }
        }
        self.draw_points(points);
    }

    pub fn draw_square(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:usize){
        let end_x:usize = if start_x + width < FRAME_BUFFER_WIDTH { start_x + width } 
            else { FRAME_BUFFER_WIDTH };
        let end_y:usize = if start_y + height < FRAME_BUFFER_HEIGHT { start_y + height } 
            else { FRAME_BUFFER_HEIGHT };       

        for x in start_x..end_x{
            for y in start_y..end_y{
                draw_pixel(x, y, color);
                //points.push(Point{x:x, y:y, color:color});
            }
        } 
       // self.draw_points(points);
    }


    fn buffer(&mut self) -> &mut Buffer {
        unsafe { self.buffer.as_mut() }
    } 

    pub fn init_frame_buffer(&mut self, virtual_address:usize) {
        if(self.start_address == 0){
            unsafe {
                self.start_address = virtual_address;
                self.buffer = Unique::new_unchecked((virtual_address) as *mut _); 
            }
            trace!("Set frame buffer address {:#x}", virtual_address);
        }
    }  
}



struct Buffer {
    chars: [[Volatile<u8>; FRAME_BUFFER_WIDTH];FRAME_BUFFER_HEIGHT],
}


