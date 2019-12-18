//! This crate contains functions to print strings in a framebuffer.
//! The coordinate in these functions is relative to the origin(top-left point) of the frame buffer.

#![no_std]

extern crate alloc;
extern crate font;
extern crate frame_buffer;
extern crate shapes;

use alloc::vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_PIXEL};
use frame_buffer::{FrameBuffer, Pixel, PixelColor};
use shapes::{Coord, Rectangle};


type ASCII = u8;

/// Prints a string in a framebuffer.
/// Returns (column, line, rectangle), i.e. the position of the next symbol and an rectangle which covers the updated area.
/// A block item (index, width) represents the index of line number and the width of charaters in this line as pixels. It can be viewed as a framebuffer block which is described in the `frame_buffer_compositor` crate.
/// # Arguments
/// * `framebuffer`: the framebuffer to display in.
/// * `coordinate`: the left top coordinate of the text block relative to the origin(top-left point) of the frame buffer.
/// * `(width, height)`: the size of the text block.
/// * `slice`: the string to display.
/// * `font_color`: the color of the text.
/// * `bg_color`: the background color of the text block.
/// * `(column, line)`: the location of the text in the text block as symbols.
pub fn print_string<P: Pixel>(
    framebuffer: &mut FrameBuffer<P>,
    coordinate: Coord,
    width: usize,
    height: usize,
    slice: &str,
    font_color: u32,
    bg_color: u32,
    column: usize,
    line: usize,
) -> (usize, usize, Rectangle) {
    let buffer_width = width / CHARACTER_WIDTH;
    let buffer_height = height / CHARACTER_HEIGHT;
    let (x, y) = (coordinate.x, coordinate.y);

    let mut curr_line = line;
    let mut curr_column = column;

    let top_left = Coord::new(0, (curr_line * CHARACTER_HEIGHT) as isize);

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
                curr_column = 0;
                curr_line += 1;
                if curr_line == buffer_height {
                    break;
                }
            }
        }
    }  

    // fill the blank of the last line
    fill_blank(
        framebuffer,
        x + (curr_column * CHARACTER_WIDTH) as isize,
        y + (curr_line * CHARACTER_HEIGHT) as isize,
        x + width as isize,
        y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
        bg_color,
    );

    let bottom_right = Coord::new(
        (buffer_width * CHARACTER_WIDTH) as isize, 
        ((curr_line + 1) * CHARACTER_HEIGHT) as isize
    );

    let update_area = Rectangle {
        top_left: top_left,
        bottom_right: bottom_right,
    };

    // fill the blank of the remaining part
    fill_blank(
        framebuffer,
        x,
        y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
        x + width as isize,
        y + height as isize,
        bg_color,
    );

    // return the position of next symbol and updated blocks.
    (curr_column, curr_line, update_area)
}

/// Prints a character to the framebuffer at position (line, column) of all characters in the text area.
/// # Arguments
/// * `framebuffer`: the framebuffer to display in.
/// * `character`: the ASCII code of the character to display.
/// * `font_color`: the color of the character.
/// * `bg_color`: the background color of the character.
/// * `coordinate`: the left top coordinate of the text block relative to the origin(top-left point) of the frame buffer.
/// * `(column, line)`: the location of the character in the text block as symbols.
pub fn print_ascii_character<P: Pixel>(
    framebuffer: &mut FrameBuffer<P>,
    character: ASCII,
    font_color: PixelColor,
    bg_color: PixelColor,
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
            framebuffer.draw_pixel(coordinate, P::from(color));
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

/// Fill a blank text area (left, top, right, bottom) with color. The tuple specifies the location of the area relative to the origin(top-left point) of the frame buffer.
pub fn fill_blank<P: Pixel>(
    framebuffer: &mut FrameBuffer<P>,
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

    let fill = vec![P::from(color); (right - left) as usize];
    let mut coordinate = Coord::new(left, top);
    
    loop {
        if coordinate.y == bottom {
            return
        }
        if let Some(start) = framebuffer.index(coordinate) {
            framebuffer.composite_buffer(&fill, start);
        }
        coordinate.y += 1;
    }
}
