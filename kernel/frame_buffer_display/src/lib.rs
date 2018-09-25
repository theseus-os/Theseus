#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(ptr_internals)]
#![feature(asm)]

extern crate spin;
extern crate acpi;

extern crate volatile;
extern crate serial_port;
extern crate kernel_config;
extern crate memory;
#[macro_use] extern crate log;
extern crate util;
extern crate alloc;
extern crate frame_buffer;


use core::ptr::Unique;
use spin::{Mutex};
use memory::{FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use core::ops::DerefMut;
use kernel_config::memory::KERNEL_OFFSET;
use alloc::boxed::Box;
use frame_buffer::{FRAME_DRAWER};

const PIXEL_BYTES:usize = 4;
const COLOR_BITS:usize = 24;

///draw a pixel in 2D compatible mode with coordinates and color.
pub fn draw_pixel(x:usize, y:usize, color:u32) {
    draw_pixel_3d(x, y, 0, color)
}

///draw a pixel in 3D mode with coordinates and color.
pub fn draw_pixel_3d(x:usize, y:usize, z:u8, color:u32) {
    let mut drawer = FRAME_DRAWER.lock();
    let (width, height) = drawer.get_resolution();
    if (x >= width || y >= height) {
        return;
    }

    let index = drawer.get_index_fn();
    let mut buffer = match drawer.buffer() {
        Ok(rs) => {rs},
        Err(err) => { debug!("Fail to get frame buffer"); return; },
    };

    draw_pixel_in_buffer(index(x, y), buffer, z, color);
}

///draw a line in 2D compatible mode with start and end coordinates and color.
pub fn draw_line(start_x:usize, start_y:usize, end_x:usize, end_y:usize,
    color:u32) {
    draw_line_3d(start_x, start_y, end_x, end_y, 0, color)
}

///draw a line in 3D mode with coordinates and color.
pub fn draw_line_3d(start_x:usize, start_y:usize, end_x:usize, end_y:usize, z:u8, 
    color:u32) {
    let (start_x, start_y, end_x, end_y) = (start_x as i32, start_y as i32, end_x as i32, end_y as i32);
    let width:i32 = end_x-start_x;
    let height:i32 = end_y-start_y;
    let mut drawer = FRAME_DRAWER.lock();
    let (buffer_width, buffer_height) = {drawer.get_resolution()};
    let index = drawer.get_index_fn();

    let mut buffer = match drawer.buffer() {
        Ok(rs) => {rs},
        Err(err) => { debug!("Fail to get frame buffer"); return; },
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
                draw_pixel_in_buffer(index(x as usize, y as usize), buffer, z, color);
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
                draw_pixel_in_buffer(index(x as usize, y as usize), buffer, z, color);
            }
            y += step;   
        }
    }
}

///draw a square in 2D compatible mode with upper left coordinates, width, height and color.
pub fn draw_rectangle(start_x:usize, start_y:usize, width:usize, height:usize,
     color:u32) {
    draw_rectangle_3d(start_x, start_y, width, height, 0, color)
}

///draw a square in 3D mode with upper left coordinates, width, height and color.
pub fn draw_rectangle_3d(start_x:usize, start_y:usize, width:usize, height:usize, z:u8,
     color:u32) {
    trace!("WEnqiu: draw in 3d");
    let mut drawer = FRAME_DRAWER.lock();
    let (buffer_width, buffer_height) = {drawer.get_resolution()};
    let index = drawer.get_index_fn();

    let mut buffer = match drawer.buffer() {
        Ok(rs) => {rs},
        Err(err) => { debug!("Fail to get frame buffer"); return; },
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
        
        draw_pixel_in_buffer(index(x, start_y), buffer, z, color);
        draw_pixel_in_buffer(index(x, end_y-1), buffer, z, color);
        x += 1;
    }

    let mut y = start_y;
    loop {
        if y == end_y {
            break;
        }

        draw_pixel_in_buffer(index(start_x, y), buffer, z, color);
        draw_pixel_in_buffer(index(end_x-1, y), buffer, z, color);
        y += 1;
    }
}

///draw a square in 2D compatible mode with upper left coordinates, width, height and color.
pub fn fill_rectangle(start_x:usize, start_y:usize, width:usize, height:usize,
     color:u32) {
    fill_rectangle_3d(start_x, start_y, width, height, 0, color)
}

///draw a square in 2D compatible mode with upper left coordinates, width, height and color.
pub fn fill_rectangle_3d(start_x:usize, start_y:usize, width:usize, height:usize, z:u8,
     color:u32) {
    let mut drawer = FRAME_DRAWER.lock();
    let (buffer_width, buffer_height) = {drawer.get_resolution()};
    let index = drawer.get_index_fn();

    let mut buffer = match drawer.buffer() {
        Ok(rs) => {rs},
        Err(err) => { debug!("Fail to get frame buffer"); return; },
    };
    
    let end_x:usize = {if start_x + width < buffer_width { start_x + width } 
        else { buffer_width }};
    let end_y:usize = {if start_y + height < buffer_height { start_y + height } 
        else { buffer_height }};  

    let mut x = start_x;
    let mut y = start_y;
//    trace!("Wenqiu:3D fill rectangle {} {} {} {} {} {} {} {}", start_x, start_y, width,  height, 
//        buffer_width, buffer_height, end_x, end_y);
    loop {
        if x == end_x {
            y += 1;
            if y == end_y {
                break;
            }
            x = start_x;
        }

        draw_pixel_in_buffer(index(x, y), buffer, z, color);
        x += 1;
    }
}

// Get the resolution of the screen
pub fn get_resolution() -> (usize, usize) {
    FRAME_DRAWER.lock().get_resolution()
}

// Check if a point is in the screen
fn check_in_range(x:usize, y:usize, width:usize, height:usize)  -> bool {
    x < width && y < height
}

fn draw_pixel_in_buffer(index:usize, buffer:&mut[u32], z:u8, color:u32) {
    if  (buffer[index] >> COLOR_BITS) <= z as u32 {
        buffer[index] = ((z as u32) << COLOR_BITS) + color;
    }
}
