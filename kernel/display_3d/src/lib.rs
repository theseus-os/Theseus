//! This crate is a frame buffer for display on the screen in 2D mode

#![no_std]
#![feature(const_fn)]
#![feature(ptr_internals)]
#![feature(asm)]

extern crate spin;
extern crate acpi;

extern crate volatile;
extern crate serial_port;
#[macro_use] extern crate log;
extern crate util;
extern crate alloc;
extern crate frame_buffer;

use spin::{Mutex};
use alloc::vec::{Vec};
use alloc::sync::Arc;

pub use frame_buffer::{FrameBuffer};

const COLOR_BITS:usize = 24;

pub trait Display {
    ///draw a 2D pixel at (x, y)
    fn draw_pixel(&mut self, x:usize, y:usize, color:u32);
    ///draw a 3D pixel at (x, y) with depth z. A pixel with a larger z will overlap others
    fn draw_pixel_3d(&mut self, x:usize, y:usize, z:u8, color:u32);
    ///draw a 2D line from (start_x, start_y) to (end_x, end_y) 
    fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32);
    ///draw a 3D line from (start_x, start_y) to (end_x, end_y) with depth z. A pixel with a larger z will overlap others
    fn draw_line_3d(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, z:u8, color:u32);
    ///draw a 2D rectangle at (start_x, start_y) 
    fn draw_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32);
    ///draw a 3D rectangle at (start_x, start_y) with depth z. A pixel with a larger z will overlap others
    fn draw_rectangle_3d(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, z:u8, color:u32);
    ///fill a 2D rectangle at (start_x, start_y)
    fn fill_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32);
    ///fill a 3D rectangle at (start_x, start_y) with depth z. A pixel with a larger z will overlap others
    fn fill_rectangle_3d(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, z:u8, color:u32);
}

impl Display for FrameBuffer {
    //draw a 2D pixel at (x, y)
    fn draw_pixel(&mut self, x:usize, y:usize, color:u32) {
        self.draw_pixel_3d(x, y, 0, color);
    }

    //draw a 3D pixel at (x, y) with depth z
    fn draw_pixel_3d(&mut self, x:usize, y:usize, z:u8, color:u32) {
        let index = self.get_index_fn();
        if self.check_in_range(x, y) {
            write_to(self.buffer(), index(x, y), z, color);
        }        
    }

    //draw a 2D line from (start_x, start_y) to (end_x, end_y) 
    fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
        self.draw_line_3d(start_x, start_y, end_x, end_y, 0, color);
    }

    //draw a 3D line from (start_x, start_y) to (end_x, end_y) with depth z
    fn draw_line_3d(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, z:u8, color:u32){
        let width:i32 = end_x - start_x;
        let height:i32 = end_y - start_y;
        
        let index = self.get_index_fn();

        //compare the x distance and y distance. Increase/Decrease the longer one at every step.
        if width.abs() > height.abs() {
            let mut y;
            let mut x = start_x;
            //if the end_x is larger than start_x, increase x. Otherwise decrease it.
            let step = if width > 0 {1} else {-1};

            loop {
                if x == end_x {
                    break;
                }          
                y = (x - start_x) * height / width +start_y;
                if self.check_in_range(x as usize,y as usize) {
                    write_to(self.buffer(), index(x as usize, y as usize), z, color);
                }
                x += step;
            }
        } else {
            let mut x;
            let mut y = start_y;
            let step = if height > 0 {1} else {-1};
            loop {
                if y == end_y {
                    break;
                }
                x = (y - start_y) * width / height + start_x;
                if { self.check_in_range(x as usize,y as usize) }{
                    write_to(self.buffer(), index(x as usize, y as usize), z, color);
                }
                y += step;   
            }
        }

    }

    //draw a 2D rectangle at (start_x, start_y) 
    fn draw_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        self.draw_rectangle_3d(start_x, start_y, width, height, 0, color);
    }

    //draw a 3D rectangle at (start_x, start_y) with depth z 
    fn draw_rectangle_3d(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, z:u8, color:u32){
        let index = self.get_index_fn();
        let (buffer_width, buffer_height) = self.get_size();

        let end_x:usize = {
            if start_x + width < buffer_width { start_x + width} 
            else { buffer_width }
        };
        let end_y:usize = {if start_y + height < buffer_height { start_y + height } 
            else { buffer_height }
        };

        let mut x = start_x;
        let buffer = self.buffer();
        loop {
            if x == end_x {
                break;
            }
            write_to(buffer, index(x, start_y), z, color);
            write_to(buffer, index(x, end_y-1), z, color);

            x += 1;
        }

        let mut y = start_y;
        loop {
            if y == end_y {
                break;
            }
            write_to(buffer, index(start_x, y), z, color);
            write_to(buffer, index(end_x-1, y), z, color);
            y += 1;
        }
    }

    //fill a 2D rectangle at (start_x, start_y) 
    fn fill_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        self.fill_rectangle_3d(start_x, start_y, width, height, 0, color);
    }

    //draw a 3D rectangle at (start_x, start_y) with depth z 
    fn fill_rectangle_3d(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, z:u8, color:u32){
        let mut x = start_x;
        let mut y = start_y;
        

        let (buffer_width, buffer_height) = self.get_size();
        let index = self.get_index_fn();
        let end_x:usize = {
            if start_x + width < buffer_width { start_x + width } 
            else { buffer_width }
        };
        let end_y:usize = {
            if start_y + height < buffer_height { start_y + height } 
            else { buffer_height }
        };  

        let buffer = self.buffer();

        loop {
            if x == end_x {
                y += 1;
                if y == end_y {
                    break;
                }
                x = start_x;
            }

            write_to(buffer, index(x, y), z, color);
            x += 1;
        }
    }

}

//write a 3D pixel to a buffer. A pixel with a larger z will overlap current pixel with a smaller z
fn write_to(buffer:&mut BoxRefMut<MappedPages, [u32]>, index:usize, z:u8, color:u32) {
    if (buffer[index] >> COLOR_BITS) <= z as u32 {
        buffer[index] = color;
    }
}

///draw a pixel at (x, y)
pub fn draw_pixel(vf:&Arc<Mutex<FrameBuffer>>, x:usize, y:usize, color:u32){
    vf.lock().draw_pixel(x, y, color);
}

///draw a line from (start_x, start_y) to (end_x, end_y) with color
pub fn draw_line(vf:&Arc<Mutex<FrameBuffer>>, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
    vf.lock().draw_line(start_x, start_y, end_x, end_y, color);
}

///draw a rectangle at (start_x, start_y) with color
pub fn draw_rectangle(vf:&Arc<Mutex<FrameBuffer>>, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
    vf.lock().draw_rectangle(start_x, start_y, width, height, color);
}

///fill a rectangle at (start_x, start_y) with color
pub fn fill_rectangle(vf:&Arc<Mutex<FrameBuffer>>, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
    vf.lock().fill_rectangle(start_x, start_y, width, height, color);
}