//! This crate contains functions to print strings in a framebuffer.

#![no_std]

extern crate alloc;
extern crate font;
extern crate frame_buffer;
extern crate frame_buffer_rgb;

use alloc::vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_PIXEL};
use frame_buffer::{FrameBuffer, AbsoluteCoord};

/// Prints a string in a framebuffer.
/// Returns (column, line) of the end, i.e. the position of the next symbol.
/// # Arguments
/// * `framebuffer`: the framebuffer to display in.
/// * `(x, y)`: the left top point of the text block.
/// * `width`: the width of the text block.
/// * `height`: the height of the text block.
/// * `slice`: the string to display.
/// * `font_color`: the color of the text.
/// * `bg_color`: the background color of the text block.
pub fn print_by_bytes(
    framebuffer: &mut dyn FrameBuffer,
    location: AbsoluteCoord,
    width: usize,
    height: usize,
    slice: &str,
    font_color: u32,
    bg_color: u32,
) -> Result<(usize, usize), &'static str> {
    let buffer_width = width / CHARACTER_WIDTH;
    let buffer_height = height / CHARACTER_HEIGHT;
    let (x, y) = location.value();

    let mut curr_line = 0;
    let mut curr_column = 0;
    for byte in slice.bytes() {
        if byte == b'\n' {
            // fill the remaining blank of current line and go to the next line
            fill_blank(
                framebuffer,
                x + curr_column * CHARACTER_WIDTH,
                y + curr_line * CHARACTER_HEIGHT,
                x + width,
                y + (curr_line + 1) * CHARACTER_HEIGHT,
                bg_color,
            )?;
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
            print_byte(
                framebuffer,
                byte,
                font_color,
                bg_color,
                x,
                y,
                curr_line,
                curr_column,
            )?;
            curr_column += 1;
        }
    }

    //fill the blank of the last line
    fill_blank(
        framebuffer,
        x + curr_column * CHARACTER_WIDTH,
        y + curr_line * CHARACTER_HEIGHT,
        x + width,
        y + (curr_line + 1) * CHARACTER_HEIGHT,
        bg_color,
    )?;

    //fill the blank of remaining lines
    fill_blank(
        framebuffer,
        x,
        y + (curr_line + 1) * CHARACTER_HEIGHT,
        x + width,
        y + height,
        bg_color,
    )?;

    // return the position of the end.
    Ok((curr_column, curr_line))
}

// print a byte to the framebuffer at (line, column) in the text area.
// (left, top) specifies the location of the text area in the framebuffer.
fn print_byte(
    framebuffer: &mut dyn FrameBuffer,
    byte: u8,
    font_color: u32,
    bg_color: u32,
    left: usize,
    top: usize,
    line: usize,
    column: usize,
) -> Result<(), &'static str> {
    let start = AbsoluteCoord::new(
        left + column * CHARACTER_WIDTH, 
        top + line * CHARACTER_HEIGHT
    );
    let fonts = FONT_PIXEL.lock();

    let mut i = 0;
    let mut j = 0;
    loop {
        let mask: u32 = fonts[byte as usize][i][j];
        let color = font_color & mask | bg_color & (!mask);
        framebuffer.draw_pixel(start + (j, i), color);
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

// fill a blank text area (left, top, right, bottom) with color.
fn fill_blank(
    framebuffer: &mut dyn FrameBuffer,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    color: u32,
) -> Result<(), &'static str> {
    if left >= right || top >= bottom {
        return Ok(());
    }

    let fill = vec![color; right - left];
    let mut line_start = AbsoluteCoord::new(left, top);
    
    loop {
        if line_start.0.y == bottom {
            return Ok(());
        }
        let start = framebuffer.index(line_start);
        framebuffer.buffer_copy(&fill, start);
        line_start = line_start + (0, 1);
    }
}
