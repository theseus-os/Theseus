//! This crate contains a series of basic draw functions to draw graphs in a framebuffer
//! Displayables invoke these basic functions to draw more compilicated graphs in a framebuffer
//! A framebuffer should be passed to the framebuffer compositor to display on the screen

#![no_std]

extern crate alloc;
extern crate frame_buffer;
extern crate font;

use alloc::vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_PIXEL};
use frame_buffer::{FrameBuffer};

/// Print a string in a framebuffer.
/// The string is printed at position (x, y) of the framebuffer. 
/// It is printed within an area specified by (width, height). The part extending the area will be ignored.
pub fn print_by_bytes(mut framebuffer: &mut FrameBuffer, 
    x: usize, 
    y: usize, 
    width: usize, 
    height: usize, 
    slice: &str, 
    font_color: u32, 
    bg_color: u32
) -> Result<(), &'static str> {
    let buffer_width = width/CHARACTER_WIDTH;
    let buffer_height = height/CHARACTER_HEIGHT;

    let mut curr_line = 0;
    let mut curr_column = 0;        
    for byte in slice.bytes() {
        if byte == b'\n' {
            // fill the remaining blank of current line and go to the next line
            fill_blank(&mut framebuffer,
                x + curr_column * CHARACTER_WIDTH,
                y + curr_line * CHARACTER_HEIGHT,
                x + width, 
                y + (curr_line + 1 )* CHARACTER_HEIGHT, 
                bg_color)?;
            curr_column = 0;
            curr_line += 1;
            if curr_line == buffer_height {
                break;
            }
        } else {
            // print the next character
            if curr_column == buffer_width {
                curr_column = 0;
                curr_line += 1;
                if curr_line == buffer_height {
                    break;
                }
            }
            print_byte(&mut framebuffer, byte, font_color, bg_color, x, y, 
                curr_line, curr_column)?;
            curr_column += 1;
        }
    }
    
    // //Fill the blank of the last line
    // fill_blank(&mut buffer,
    //     x + curr_column * CHARACTER_WIDTH,
    //     y + curr_line * CHARACTER_HEIGHT,
    //     x + width, 
    //     y + (curr_line + 1 )* CHARACTER_HEIGHT, 
    //     bg_color, &index)?;

    // Fill the blank of remaining lines
    // fill_blank(&mut buffer, 
    //     x, y + (curr_line + 1 )* CHARACTER_HEIGHT, x + width, y + height, 
    //     bg_color, &index)?;

    Ok(())
}

// print a byte to the framebuffer buffer at (line, column) in the text area. 
// (left, top) specifies the location of the text area in the framebuffer. 
fn print_byte(framebuffer: &mut FrameBuffer, byte: u8, font_color: u32, bg_color: u32,
        left: usize, top: usize, line: usize, column: usize) 
        -> Result<(),&'static str> {
    let x = left + column * CHARACTER_WIDTH;
    let y = top + line * CHARACTER_HEIGHT;
    let fonts = FONT_PIXEL.lock();

    let mut i = 0;
    let mut j = 0;
    loop {
        let mask: u32 = fonts[byte as usize][i][j];
        let index = framebuffer.index(x + j, y + i);
        framebuffer.buffer_mut()[index] = font_color & mask | bg_color & (!mask);
        j += 1;
        if j == CHARACTER_WIDTH {
            i += 1;
            if i == CHARACTER_HEIGHT {
                return Ok(());
            }
            j = 0;
        }
    }
}

// Fill a blank text area (left, top, right, bottom) with the color.
fn fill_blank(framebuffer: &mut FrameBuffer, left: usize, top: usize, right: usize,
            bottom: usize, color: u32) -> Result<(),&'static str> {
    if left >= right || top >= bottom {
        return Ok(())
    }

    let fill = vec![color; right - left];
    let mut y = top;
    loop {
        if y == bottom {
            return Ok(());
        }
        let start = framebuffer.index(left, y);
        let end = framebuffer.index(right, y);
        framebuffer.buffer_mut()[start..end].copy_from_slice(&fill);
        y += 1;
    }
}