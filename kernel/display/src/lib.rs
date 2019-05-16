//! This crate is a frame buffer for display on the screen in 2D mode

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

use frame_buffer::{PhysicalFrameBuffer,draw_in_buffer, check_in_range, get_buffer_ref};

//The buffer for text printing
pub mod text_buffer;
//The font for text printing
pub mod font;

const PIXEL_BYTES:usize = 4;

// #[cfg(framebuffer3d)]
// const COLOR_BITS:usize = 24;

// The drawer is responsible for drawing/printing to the screen
pub struct VirtualFrameBuffer {
    x: usize,
    y: usize,
    width:usize,
    height:usize,
    physical_buffer:PhysicalFrameBuffer
}



impl VirtualFrameBuffer {
    pub fn new(x:usize, y:usize, width:usize, height:usize) -> Result<VirtualFrameBuffer, &'static str>{
        let buffer_ref = get_buffer_ref()?;
        Ok(VirtualFrameBuffer {
            x:x,
            y:y,
            width:width,
            height:height,
            physical_buffer:buffer_ref
        })
    }

    //get the resolution of the screen
    pub fn get_resolution(&self) -> (usize, usize) {
        (self.width, self.height)
        //frame_buffer::FRAME_BUFFER.lock().get_resolution()
    }

    pub fn draw_pixel(&self, x:usize, y:usize, color:u32) {
        //let buffer = self.physical_buffer.deref();


        let index = self.physical_buffer.get_index_fn();
        let abs_x = self.x + x;
        let abs_y = self.y + y;
        let mut buffer = self.physical_buffer.buffer_ref.lock();
        if self.physical_buffer.check_in_range(abs_x, abs_y) {
            buffer[index(abs_x, abs_y)] = color;
        }
        
    }

    pub fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        
        let abs_start_x = start_x + self.x as i32;
        let abs_start_y = start_y + self.y as i32;
        let abs_end_x = end_x + self.x as i32;
        let abs_end_y = end_y + self.y as i32;

        let index = self.physical_buffer.get_index_fn();
        // let mut fb = frame_buffer::FRAME_BUFFER.lock();
        // let (buffer_width, buffer_height) = fb.get_resolution();
        // let index = fb.get_index_fn();
        // let buffer = match fb.buffer(){
        //     Ok(rs) => {rs},
        //     Err(e) => { error!("Fail to get buffer: {}", e); return; }
        // };
        let mut buffer = self.physical_buffer.buffer_ref.lock();
        if width.abs() > height.abs() {
            let mut y;
            let mut x = abs_start_x;
            let step = if width > 0 {1} else {-1};

            loop {
                if x == abs_end_x {
                    break;
                }          
                y = (x - abs_start_x) * height / width +abs_start_y;
                if self.physical_buffer.check_in_range(x as usize,y as usize) {
                    buffer[index(x as usize, y as usize)] = color;
                }
                x += step;
            }
        } else {
            let mut x;
            let mut y = abs_start_y;
            let step = if height > 0 {1} else {-1};
            loop {
                if y == abs_end_y {
                    break;
                }
                x = (y - abs_start_y) * width / height + abs_start_x;
                if self.physical_buffer.check_in_range(x as usize,y as usize) {
                    buffer[index(x as usize, y as usize)] =  color;
                }
                y += step;   
            }
        }

    }

    pub fn draw_rectangle(&self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        //let mut fb = frame_buffer::FRAME_BUFFER.lock();
        let index = self.physical_buffer.get_index_fn();
        let (buffer_width, buffer_height) = self.physical_buffer.get_resolution();
        // let buffer = match fb.buffer(){
        //     Ok(rs) => {rs},
        //     Err(e) => { error!("Fail to get buffer: {}", e); return; }
        // };

        let abs_start_x = start_x + self.x;
        let abs_start_y = start_y + self.y;
        let abs_end_x:usize = {if start_x + width+self.x < buffer_width 
            { start_x + width + self.x } 
            else { buffer_width }};
        let abs_end_y:usize = {if start_y + height + self.y < buffer_height 
            { start_y + height + self.y } 
            else { buffer_height }};

        let mut x = abs_start_x;
        let mut buffer = self.physical_buffer.buffer_ref.lock();
        loop {
            if x == abs_end_x {
                break;
            }
            buffer[index(self.x, abs_start_y)] = color;
            buffer[index(self.x, abs_end_y-1)] = color;
            // buffer[index(x, start_y)] = color;
            // buffer[index(x, end_y-1)] = color;
            x += 1;
        }

        let mut y = abs_start_y;
        loop {
            if y == abs_end_y {
                break;
            }
            buffer[index(abs_start_x, y)] = color;
            buffer[index(abs_end_x-1, y)] = color;
            // buffer[index(start_x, y)] = color;
            // buffer[index(end_x-1, y)] = color;
            y += 1;
        }
    }

    pub fn fill_rectangle(&self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        let abs_start_x = start_x + self.x;
        let abs_start_y = start_y + self.y;

        let mut x = abs_start_x;
        let mut y = abs_start_y;

        // let mut fb = frame_buffer::FRAME_BUFFER.lock();
        let (buffer_width, buffer_height) = self.physical_buffer.get_resolution();
        let index = self.physical_buffer.get_index_fn();
        // let buffer = match fb.buffer(){
        //     Ok(rs) => {rs},
        //     Err(e) => { error!("Fail to get buffer: {}", e); return; }
        // };


        let abs_end_x:usize = {if start_x + width + self.x < buffer_width 
            { start_x + width + self.x} 
            else { buffer_width }};
        let abs_end_y:usize = {if start_y + height + self.y < buffer_height 
            { start_y + height + self.y} 
            else { buffer_height }};  

        let mut buffer = self.physical_buffer.buffer_ref.lock();
        loop {
            if x == abs_end_x {
                y += 1;
                if y == abs_end_y {
                    break;
                }
                x = abs_start_x;
            }

            buffer[index(x, y)] = color;
            x += 1;
        }
    }

}