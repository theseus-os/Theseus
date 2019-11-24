//! This crate contains functions to print strings in a framebuffer.
//! The coordinate in these functions is relative to the origin(top-left point) of the frame buffer.

#![no_std]

extern crate alloc;
extern crate font;
extern crate frame_buffer;
extern crate frame_buffer_rgb;

use alloc::vec;
use alloc::vec::Vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_PIXEL};
use frame_buffer::{FrameBuffer, Coord};

type ASCII = u8;

/// Prints a string in a framebuffer.
/// Returns (column, line, blocks), i.e. the position of the next symbol and the information of updated blocks.
/// A block item (index, width) represents the index of line number and the width of charaters in this line as pixels. It can be viewed as a framebuffer block which is described in the `frame_buffer_compositor` crate.
/// # Arguments
/// * `framebuffer`: the framebuffer to display in.
/// * `coordinate`: the left top coordinate of the text block relative to the origin(top-left point) of the frame buffer.
/// * `(width, height)`: the size of the text block.
/// * `slice`: the string to display.
/// * `font_color`: the color of the text.
/// * `bg_color`: the background color of the text block.
/// * `(column, line)`: the location of the text in the text block as symbols.
pub fn print_string(
    framebuffer: &mut dyn FrameBuffer,
    coordinate: Coord,
    width: usize,
    height: usize,
    slice: &str,
    font_color: u32,
    bg_color: u32,
    column: usize,
    line: usize,
) -> (usize, usize, Vec<(usize, usize, usize)>) {
    let buffer_width = width / CHARACTER_WIDTH;
    let buffer_height = height / CHARACTER_HEIGHT;
    let (x, y) = (coordinate.x, coordinate.y);

    let mut curr_line = line;
    let mut curr_column = column;

    let mut blocks = Vec::new();

    for byte in slice.bytes() {
        if byte == b'\n' {
            // fill the remaining blank of current line and go to the next line
            fill_blank(
                framebuffer,
                coordinate.x + (curr_column * CHARACTER_WIDTH) as isize,
                coordinate.y + (curr_line * CHARACTER_HEIGHT) as isize,
                coordinate.x + width as isize,
                coordinate.y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
                bg_color,
            );
            blocks.push((curr_line, 0, curr_column  * CHARACTER_WIDTH));
            curr_column = 0;
            curr_line += 1;
            if curr_line == buffer_height {
                break;
            }
        } else {
            // print the next character
            print_ascii_character(
                framebuffer,
                byte,
                font_color,
                bg_color,
                coordinate,
                curr_column,
                curr_line,
            );
            curr_column += 1;
            if curr_column == buffer_width {
                blocks.push((curr_line, 0, buffer_width));
                curr_column = 0;
                curr_line += 1;
                if curr_line == buffer_height {
                    break;
                }
            }
        }
    }    
    //fill the blank of the last line
    blocks.push((curr_line, 0, curr_column * CHARACTER_WIDTH));
    fill_blank(
        framebuffer,
        x + (curr_column * CHARACTER_WIDTH) as isize,
        y + (curr_line * CHARACTER_HEIGHT) as isize,
        x + width as isize,
        y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
        bg_color,
    );

    // // fill the next line in case the page scrolls up
    // blocks.push((curr_line + 1, 0));
    // fill_blank(
    //     framebuffer,
    //     x,
    //     y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
    //     x + width as isize,
    //     y + ((curr_line + 2) * CHARACTER_HEIGHT) as isize,
    //     bg_color,
    // );

    // Fill the remaining lines if the offset is zero and the text displayable refreshes from the beginning.
    // If the offset is not zero, it means the text to be printed is appended and just refresh the part occupied by the new text.
    // In the future we may adjust the logic here to for the optimization of more displayables and applications
    if column == 0 && line == 0 {
        for i in (curr_line + 1)..(height - 1) / CHARACTER_HEIGHT + 1 {
            blocks.push((i, 0, 0));
        }
        // fill the blank of remaining lines
        fill_blank(
            framebuffer,
            x,
            y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
            x + width as isize,
            y + height as isize,
            bg_color,
        );
    }

    // return the position of next symbol and updated blocks.
    (curr_column, curr_line, blocks)
}

/// Prints a character to the framebuffer at position (line, column) of all characters in the text area.
/// # Arguments
/// * `framebuffer`: the framebuffer to display in.
/// * `character`: the ASCII code of the character to display.
/// * `font_color`: the color of the character.
/// * `bg_color`: the background color of the character.
/// * `coordinate`: the left top coordinate of the text block relative to the origin(top-left point) of the frame buffer.
/// * `(column, line)`: the location of the character in the text block as symbols.
pub fn print_ascii_character(
    framebuffer: &mut dyn FrameBuffer,
    character: ASCII,
    font_color: u32,
    bg_color: u32,
    coordinate: Coord,
    column: usize,
    line: usize,
) {
    let start = coordinate + ((column * CHARACTER_WIDTH) as isize, (line * CHARACTER_HEIGHT) as isize);
    if !framebuffer.overlaps_with(start, CHARACTER_WIDTH, CHARACTER_HEIGHT) {
        return
    }

    let fonts = FONT_PIXEL.lock();

    // print from the offset within the frame buffer
    let (buffer_width, buffer_height) = framebuffer.get_size();
    let off_set_x: usize = if start.x < 0 { -(start.x) as usize } else { 0 };
    let off_set_y: usize = if start.y < 0 { -(start.y) as usize } else { 0 };    
    let mut j = off_set_x;
    let mut i = off_set_y;
    loop {
        let coordinate = start + (j as isize, i as isize);
        if framebuffer.contains(coordinate) {
            let mask: u32 = fonts[character as usize][i][j];
            let color = font_color & mask | bg_color & (!mask);
            framebuffer.draw_pixel(coordinate, color);
        }
        j += 1;
        if j == CHARACTER_WIDTH || start.x + j as isize == buffer_width as isize {
            i += 1;
            if i == CHARACTER_HEIGHT || start.y + i as isize == buffer_height as isize {
                return
            }
            j = off_set_x;
        }
    }
}

// fill a blank text area (left, top, right, bottom) with color. The tuple specifies the location of the area relative to the origin(top-left point) of the frame buffer.
fn fill_blank(
    framebuffer: &mut dyn FrameBuffer,
    left: isize,
    top: isize,
    right: isize,
    bottom: isize,
    color: u32,
) {

    let (width, height) = framebuffer.get_size();
    // fill the part within the frame buffer
    let left = core::cmp::max(0, left);
    let right = core::cmp::min(right, width as isize);
    let top = core::cmp::max(0, top);
    let bottom = core::cmp::min(bottom, height as isize);

    if left >= right || top >= bottom {
        return
    }

    let fill = vec![color; (right - left) as usize];
    let mut coordinate = Coord::new(left, top);
    
    loop {
        if coordinate.y == bottom {
            return
        }
        if let Some(start) = framebuffer.index(coordinate) {
            framebuffer.buffer_copy(&fill, start);
        }
        coordinate.y += 1;
    }
}
