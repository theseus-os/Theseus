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

use frame_buffer::{draw_in_buffer, check_in_range};

//The buffer for text printing
pub mod text_buffer;
//The font for text printing
pub mod font;

const PIXEL_BYTES:usize = 4;

// #[cfg(framebuffer3d)]
// const COLOR_BITS:usize = 24;

// The drawer is responsible for drawing/printing to the screen
pub struct Drawer {

}



impl Drawer {
    /*unsafe fn set_background(&mut self, offset:usize, len:usize, color:u32) {
        asm!("cld
            rep stosd"
            :
            : "{rdi}"(self.start_address + offset), "{eax}"(color), "{rcx}"(len)
            : "cc", "memory", "rdi", "rcx"
            : "intel", "volatile");
    }*/


    //get the resolution of the screen
    pub fn get_resolution(&self) -> (usize, usize) {
        frame_buffer::FRAME_BUFFER.lock().get_resolution()
    }

    pub fn draw_pixel(&self, x:usize, y:usize, color:u32) {
        let mut fb = frame_buffer::FRAME_BUFFER.lock();
        let index = fb.get_index_fn();
        let (buffer_width, buffer_height) = fb.get_resolution();
        let buffer = match fb.buffer(){
            Ok(rs) => {rs},
            Err(e) => { error!("Fail to get buffer: {}", e); return; }
        };

        if check_in_range(x as usize,y as usize, buffer_width, buffer_height) {
            draw_in_buffer(index(x, y), color, buffer);
        }
    }

    pub fn draw_line(&self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        let mut fb = frame_buffer::FRAME_BUFFER.lock();
        let (buffer_width, buffer_height) = fb.get_resolution();
        let index = fb.get_index_fn();
        let buffer = match fb.buffer(){
            Ok(rs) => {rs},
            Err(e) => { error!("Fail to get buffer: {}", e); return; }
        };

        if width.abs() > height.abs() {
            let mut y;
            let mut x = start_x;
            let step = if width > 0 {1} else {-1};

            loop {
                if x == end_x {
                    break;
                }          
                y = (x - start_x) * height / width + start_y;
                if check_in_range(x as usize,y as usize, buffer_width, buffer_height) {
                    draw_in_buffer(index(x as usize, y as usize), color, buffer);
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
                if check_in_range(x as usize,y as usize, buffer_width, buffer_height) {
                    draw_in_buffer(index(x as usize, y as usize), color, buffer);
                }
                y += step;   
            }
        }

    }

    pub fn draw_rectangle(&self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        let mut fb = frame_buffer::FRAME_BUFFER.lock();
        let index = fb.get_index_fn();
        let (buffer_width, buffer_height) = fb.get_resolution();
        let buffer = match fb.buffer(){
            Ok(rs) => {rs},
            Err(e) => { error!("Fail to get buffer: {}", e); return; }
        };

        let end_x:usize = {if start_x + width < buffer_width { start_x + width } 
            else { buffer_width }};
        let end_y:usize = {if start_y + height < buffer_height { start_y + height } 
            else { buffer_height }};

        let mut x = start_x;
        loop {
            if x == end_x {
                break;
            }
            draw_in_buffer(index(x, start_y), color, buffer);
            draw_in_buffer(index(x, end_y-1), color, buffer);
            // buffer[index(x, start_y)] = color;
            // buffer[index(x, end_y-1)] = color;
            x += 1;
        }

        let mut y = start_y;
        loop {
            if y == end_y {
                break;
            }
            draw_in_buffer(index(start_x, y), color, buffer);
            draw_in_buffer(index(end_x-1, y), color, buffer);
            // buffer[index(start_x, y)] = color;
            // buffer[index(end_x-1, y)] = color;
            y += 1;
        }
    }

    pub fn fill_rectangle(&self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        
        let mut x = start_x;
        let mut y = start_y;

        let mut fb = frame_buffer::FRAME_BUFFER.lock();
        let (buffer_width, buffer_height) = fb.get_resolution();
        let index = fb.get_index_fn();
        let buffer = match fb.buffer(){
            Ok(rs) => {rs},
            Err(e) => { error!("Fail to get buffer: {}", e); return; }
        };


        let end_x:usize = {if start_x + width < buffer_width { start_x + width } 
            else { buffer_width }};
        let end_y:usize = {if start_y + height < buffer_height { start_y + height } 
            else { buffer_height }};  

        loop {
            if x == end_x {
                y += 1;
                if y == end_y {
                    break;
                }
                x = start_x;
            }

            draw_in_buffer(index(x, y), color, buffer);
            x += 1;
        }
    }

}