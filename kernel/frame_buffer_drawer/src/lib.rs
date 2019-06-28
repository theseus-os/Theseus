//! This crate contains a series of basic draw functions to draw in a framebuffer.
//! Displayables invoke these basic functions to draw more compilicated graphs in a framebuffer.
//! A framebuffer should be passed to the framebuffer compositor to display on the screen.

#![no_std]

extern crate alloc;
extern crate frame_buffer;
extern crate frame_buffer_pixel_drawer;
extern crate font;

use alloc::vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_PIXEL};
use frame_buffer::{FrameBuffer};
use frame_buffer_pixel_drawer::draw_pixel;

/// Draw a pixel in a framebuffer.
/// The pixel is drawed at position (x, y) of the framebuffer with color
pub fn draw_point(mut framebuffer: &mut FrameBuffer, x: usize, y: usize, color: u32){    
    if framebuffer.check_in_range(x, y) {
        draw_pixel(&mut framebuffer, x, y, color);
    }
}

/// Draw a line in a framebuffer.
/// The start point of the line is(start_x, start_y) and the end point is (end_x, end_y)
/// The part exceeding the boundary of the framebuffer will be ignored.
pub fn draw_line(mut framebuffer: &mut FrameBuffer, start_x: i32, start_y: i32, end_x: i32, end_y: i32, color: u32){
    let width: i32 = end_x - start_x;
    let height: i32 = end_y - start_y;

    // compare the x distance and y distance. Increase/Decrease the longer one at every step.
    if width.abs() > height.abs() {
        let mut y;
        let mut x = start_x;

        // if the end_x is larger than start_x, increase x in the loop. Otherwise decrease it.
        let step = if width > 0 { 1 } else { -1 };
        loop {
            if x == end_x {
                break;
            }          
            y = (x - start_x) * height / width + start_y;
            if framebuffer.check_in_range(x as usize, y as usize) {
                draw_pixel(&mut framebuffer, x as usize, y as usize, color);
            }
            x += step;
        }
    } else {
        let mut x;
        let mut y = start_y;
        let step = if height > 0 { 1 } else { -1 };
        loop {
            if y == end_y {
                break;
            }
            x = (y - start_y) * width / height + start_x;
            if { framebuffer.check_in_range(x as usize,y as usize) }{
                draw_pixel(&mut framebuffer, x as usize, y as usize, color);
            }
            y += step;   
        }
    }
}

/// Draw a rectangle in a framebuffer.
/// The left top point of the rectangle is (start_x, start_y).
/// The part exceeding the boundary of the framebuffer will be ignored.
pub fn draw_rectangle(mut framebuffer: &mut FrameBuffer, start_x: usize, start_y: usize, width: usize, height: usize, color: u32){
    let (buffer_width, buffer_height) = framebuffer.get_size();
    let end_x: usize = { 
        if start_x + width < buffer_width { start_x + width } 
        else { buffer_width }
    };
    let end_y: usize = {
        if start_y + height < buffer_height { start_y + height } 
        else { buffer_height }
    };

    let mut x = start_x;
    loop {
        if x == end_x {
            break;
        }
        draw_pixel(&mut framebuffer, x as usize, start_y as usize, color);
        draw_pixel(&mut framebuffer, x as usize, end_y - 1 as usize, color);
        x += 1;
    }

    let mut y = start_y;
    loop {
        if y == end_y {
            break;
        }
        draw_pixel(&mut framebuffer, start_x as usize, y as usize, color);
        draw_pixel(&mut framebuffer, end_x - 1 as usize, y as usize, color);
        y += 1;
    }
}

///Fill a rectangle in a framebuffer with color.
///The left top point of the rectangle is (start_x, start_y).
///The part exceeding the boundary of the framebuffer will be ignored.
pub fn fill_rectangle(mut framebuffer: &mut FrameBuffer, start_x: usize, start_y: usize, width: usize, height: usize, color: u32){
    let (buffer_width, buffer_height) = framebuffer.get_size();
   
    let end_x: usize = {
        if start_x + width < buffer_width { start_x + width } 
        else { buffer_width }
    };
    let end_y: usize = {
        if start_y + height < buffer_height { start_y + height } 
        else { buffer_height }
    }; 

    let mut x = start_x;
    let mut y = start_y;
    loop {
        loop {
            draw_pixel(&mut framebuffer, x as usize, y as usize, color);
            x += 1;
            if x == end_x {
                break;
            }
        }
        y += 1;
        if y == end_y {
            break;
        }
        x = start_x;
    }
}