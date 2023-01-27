//! This crate contains functions to print strings in a framebuffer.
//! The coordinate in these functions is relative to the origin(top-left point) of the framebuffer.

#![no_std]

extern crate alloc;
extern crate font;
extern crate framebuffer;
extern crate shapes;

use alloc::vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use framebuffer::{Framebuffer, Pixel};
use shapes::{Coord, Rectangle};


type Ascii = u8;

/// Prints a string in a framebuffer.
/// Returns (column, line, rectangle), i.e. the position of the next symbol and an rectangle which covers the updated area.
/// A block item (index, width) represents the index of line number and the width of charaters in this line as pixels. It can be viewed as a framebuffer block which is described in the `framebuffer_compositor` crate.
/// # Arguments
/// * `framebuffer`: the framebuffer to display in.
/// * `coordinate`: the left top coordinate of the text block relative to the origin(top-left point) of the framebuffer.
/// * `width`, `height`: the size of the text block in number of pixels.
/// * `slice`: the string to display.
/// * `fg_pixel`: the value of pixels in the foreground.
/// * `bg_pixel` the value of pixels in the background.
/// * `column`, `line`: the location of the text in the text block in number of characters.
#[allow(clippy::too_many_arguments)]
pub fn print_string<P: Pixel>(
    framebuffer: &mut Framebuffer<P>,
    coordinate: Coord,
    width: usize,
    height: usize,
    slice: &str,
    fg_pixel: P,
    bg_pixel: P,
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
            let mut blank = Rectangle {
                top_left: Coord::new(
                    coordinate.x + (curr_column * CHARACTER_WIDTH) as isize,
                    coordinate.y + (curr_line * CHARACTER_HEIGHT) as isize,
                ),
                bottom_right: Coord::new(
                    coordinate.x + width as isize,
                    coordinate.y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
                )
            };
            // fill the remaining blank of current line and go to the next line
            fill_blank(
                framebuffer,
                &mut blank,
                bg_pixel,
            );
            curr_column = 0;
            curr_line += 1;
            if curr_line == buffer_height {
                break;
            }
        } else {
            if curr_column == buffer_width {
                curr_column = 0;
                curr_line += 1;
                if curr_line == buffer_height {
                    break;
                }
            }
            // print the next character
            print_ascii_character(
                framebuffer,
                byte,
                fg_pixel,
                bg_pixel,
                coordinate,
                curr_column,
                curr_line,
            );
            curr_column += 1;
        }
    }  

    let mut blank = Rectangle {
        top_left: Coord::new(
            x + (curr_column * CHARACTER_WIDTH) as isize,
            y + (curr_line * CHARACTER_HEIGHT) as isize,
        ),
        bottom_right: Coord::new(
            x + width as isize,
            y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
        )
    };
    // fill the blank of the last line
    fill_blank(
        framebuffer,
        &mut blank,
        bg_pixel,
    );

    let bottom_right = Coord::new(
        (buffer_width * CHARACTER_WIDTH) as isize, 
        ((curr_line + 1) * CHARACTER_HEIGHT) as isize
    );

    let update_area = Rectangle {
        top_left,
        bottom_right,
    };

    // fill the blank of the remaining part
    blank = Rectangle {
        top_left: Coord::new(
            x,
            y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
        ),
        bottom_right: Coord::new(
            x + width as isize,
            y + height as isize,
        )
    };
    fill_blank(
        framebuffer,
        &mut blank,
        bg_pixel,
    );

    // return the position of next symbol and updated blocks.
    (curr_column, curr_line, update_area)
}

/// Prints a character to the framebuffer at position (line, column) of all characters in the text area.
/// # Arguments
/// * `framebuffer`: the framebuffer to display in.
/// * `character`: the ASCII code of the character to display.
/// * `fg_pixel`: the value of every pixel in the character.
/// * `bg_color`: the value of every pixel in the background.
/// * `coordinate`: the left top coordinate of the text block relative to the origin(top-left point) of the framebuffer.
/// * `column`, `line`: the location of the character in the text block as symbols.
pub fn print_ascii_character<P: Pixel>(
    framebuffer: &mut Framebuffer<P>,
    character: Ascii,
    fg_pixel: P,
    bg_pixel: P,
    coordinate: Coord,
    column: usize,
    line: usize,
) {
    let start = coordinate + ((column * CHARACTER_WIDTH) as isize, (line * CHARACTER_HEIGHT) as isize);
    if !framebuffer.overlaps_with(start, CHARACTER_WIDTH, CHARACTER_HEIGHT) {
        return
    }
    // print from the offset within the framebuffer
    let (buffer_width, buffer_height) = framebuffer.get_size();
    let off_set_x: usize = if start.x < 0 { -(start.x) as usize } else { 0 };
    let off_set_y: usize = if start.y < 0 { -(start.y) as usize } else { 0 };    
    let mut j = off_set_x;
    let mut i = off_set_y;
    loop {
        let coordinate = start + (j as isize, i as isize);
        if framebuffer.contains(coordinate) {
            let pixel = if j >= 1 {
                // leave 1 pixel gap between two characters
                let index = j - 1;
                let char_font = font::FONT_BASIC[character as usize][i];
                if get_bit(char_font, index) != 0 {
                    fg_pixel
                } else {
                    bg_pixel
                }
            } else {
                bg_pixel
            };
            framebuffer.draw_pixel(coordinate, pixel);
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

/// Fill a blank text area (left, top, right, bottom) with color. The tuple specifies the location of the area relative to the origin(top-left point) of the framebuffer.
pub fn fill_blank<P: Pixel>(
    framebuffer: &mut Framebuffer<P>,
    blank: &mut Rectangle,
    pixel: P,
) {

    let (width, height) = framebuffer.get_size();
    // fill the part within the framebuffer
    blank.top_left.x = core::cmp::max(0, blank.top_left.x);
    blank.top_left.y = core::cmp::max(0, blank.top_left.y);
    blank.bottom_right.x = core::cmp::min(blank.bottom_right.x, width as isize);
    blank.bottom_right.y = core::cmp::min(blank.bottom_right.y, height as isize);

    if blank.top_left.x >= blank.bottom_right.x || 
        blank.top_left.y >= blank.bottom_right.y {
        return
    }

    let fill = vec![pixel; (blank.bottom_right.x - blank.top_left.x) as usize];
    let mut coordinate = blank.top_left;    
    loop {
        if coordinate.y == blank.bottom_right.y {
            return
        }
        if let Some(start) = framebuffer.index_of(coordinate) {
            framebuffer.composite_buffer(&fill, start);
        }
        coordinate.y += 1;
    }
}

/// Gets the i_th most significant bit of `char_font`. The returned value is `1` or `0`.
fn get_bit(char_font: u8, i: usize) -> u8 {
    char_font & (0x80 >> i)
}

