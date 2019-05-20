//! This crate is to display in a virtual frame buffer in 2D mode

#![no_std]
#![feature(const_fn)]
#![feature(ptr_internals)]
#![feature(asm)]

extern crate spin;
extern crate acpi;

extern crate volatile;
extern crate serial_port;
extern crate memory;
#[macro_use] extern crate log;
extern crate util;
extern crate alloc;
extern crate frame_buffer_3d;
extern crate frame_buffer;

use spin::{Mutex};
use memory::{FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use core::ops::DerefMut;
use alloc::boxed::Box;
use alloc::sync::Arc;

use frame_buffer::{VirtualFrameBuffer};

const PIXEL_BYTES:usize = 4;


///This trait is to display graphs in a virtual frame buffer
pub trait Display {
    ///draw a pixel at (x, y) with color
    fn draw_pixel(&mut self, x:usize, y:usize, color:u32);
    ///draw a line from (start_x, start_y) to (end_x, end_y) with color
    fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32);
    ///draw a rectangle at (start_x, start_y) with color
    fn draw_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32);
    ///fill a rectangle at (start_x, start_y) with color
    fn fill_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32);
}

impl Display for VirtualFrameBuffer {
    fn draw_pixel(&mut self, x:usize, y:usize, color:u32) {
        //let buffer = self.physical_buffer.deref();


        let index = self.get_index_fn();
        if self.check_in_range(x, y) {
            self.buffer()[index(x, y)] = color;
        }
        
    }

    fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        
        let index = self.get_index_fn();
        // let mut fb = frame_buffer::FRAME_BUFFER.lock();
        // let (buffer_width, buffer_height) = fb.get_resolution();
        // let index = fb.get_index_fn();
        // let buffer = match fb.buffer(){
        //     Ok(rs) => {rs},
        //     Err(e) => { error!("Fail to get buffer: {}", e); return; }
        // };

        if width.abs() > height.abs() {
            let mut y;
            let mut x = start_x;
            let step = if width > 0 {1} else {-1};

            loop {
                if x == end_x {
                    break;
                }          
                y = (x - start_x) * height / width +start_y;
                if self.check_in_range(x as usize,y as usize) {
                    self.buffer()[index(x as usize, y as usize)] = color;
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
                    self.buffer()[index(x as usize, y as usize)] =  color;
                }
                y += step;   
            }
        }

    }

    fn draw_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        let index = self.get_index_fn();
        let (buffer_width, buffer_height) = self.get_size();

        let end_x:usize = {if start_x + width < buffer_width 
            { start_x + width} 
            else { buffer_width }};
        let end_y:usize = {if start_y + height < buffer_height 
            { start_y + height } 
            else { buffer_height }};

        let mut x = start_x;
        let mut buffer = self.buffer();
        loop {
            if x == end_x {
                break;
            }
            buffer[index(x, start_y)] = color;
            buffer[index(x, end_y-1)] = color;
            x += 1;
        }

        let mut y = start_y;
        loop {
            if y == end_y {
                break;
            }
            buffer[index(start_x, y)] = color;
            buffer[index(end_x-1, y)] = color;
            y += 1;
        }
    }

    fn fill_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        let mut x = start_x;
        let mut y = start_y;
        

        let (buffer_width, buffer_height) = self.get_size();
        let index = self.get_index_fn();
        let end_x:usize = {if start_x + width < buffer_width 
            { start_x + width } 
            else { buffer_width }};
        let end_y:usize = {if start_y + height < buffer_height 
            { start_y + height } 
            else { buffer_height }};  

        let buffer = self.buffer();

      loop {
            if x == end_x {
                y += 1;
                if y == end_y {
                    break;
                }
                x = start_x;
            }

            buffer[index(x, y)] = color;
            x += 1;
        }
    }

}

pub fn draw_pixel(vf:&Arc<Mutex<VirtualFrameBuffer>>, x:usize, y:usize, color:u32){
    vf.lock().draw_pixel(x, y, color);
}

pub fn draw_line(vf:&Arc<Mutex<VirtualFrameBuffer>>, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
    vf.lock().draw_line(start_x, start_y, end_x, end_y, color);
}

pub fn draw_rectangle(vf:&Arc<Mutex<VirtualFrameBuffer>>, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
    vf.lock().draw_rectangle(start_x, start_y, width, height, color);
}

pub fn fill_rectangle(vf:&Arc<Mutex<VirtualFrameBuffer>>, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
    vf.lock().fill_rectangle(start_x, start_y, width, height, color);
}