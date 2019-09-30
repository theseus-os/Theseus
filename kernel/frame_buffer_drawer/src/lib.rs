//! This crate contains a series of basic draw functions to draw in a framebuffer.
//! Displayables invoke these basic functions to display themselves in a framebuffer.

#![no_std]

extern crate frame_buffer;

use frame_buffer::{FrameBuffer, AbsoluteCoord, ICoord};

/// Draws a point in a framebuffer.
/// The point is drawn at position (x, y) of the framebuffer with color.
pub fn draw_point(framebuffer: &mut dyn FrameBuffer, location: AbsoluteCoord, color: u32) {
    if framebuffer.check_in_buffer(location) {
        framebuffer.draw_pixel(location, color);
    }
}

/// Draws a line in a framebuffer. The part exceeding the boundary of the framebuffer will be ignored.
/// # Arguments
/// * `framebuffer`: the framebuffer to draw in.
/// * `(start_x, start_y)`: the start point of the line.
/// * `(end_x, end_y)`: the end point of the line.
/// * `color`: the color of the line.
pub fn draw_line(
    framebuffer: &mut dyn FrameBuffer,
    start: ICoord,
    end: ICoord,
    color: u32,
) {
    let width: i32 = end.x - start.x;
    let height: i32 = end.y - start.y;

    // compare the x distance and y distance. Increase/Decrease the longer one at every step.
    if width.abs() > height.abs() {
        let mut y;
        let mut x = start.x;

        // if the end.x is larger than start.x, increase x in the loop. Otherwise decrease it.
        let step = if width > 0 { 1 } else { -1 };
        loop {
            if x == end.x {
                break;
            }
            y = (x - start.x) * height / width + start.y;
            let location = AbsoluteCoord::new(x as usize, y as usize);
            if framebuffer.check_in_buffer(location) {
                framebuffer.draw_pixel(location, color);
            }
            x += step;
        }
    } else {
        let mut x;
        let mut y = start.y;
        let step = if height > 0 { 1 } else { -1 };
        loop {
            if y == end.y {
                break;
            }
            x = (y - start.y) * width / height + start.x;
            let location = AbsoluteCoord::new(x as usize, y as usize);
            if { framebuffer.check_in_buffer(location) } {
                framebuffer.draw_pixel(location, color);
            }
            y += step;
        }
    }
}

/// Draws a rectangle in a framebuffer.
/// The part exceeding the boundary of the framebuffer will be ignored.
/// # Arguments
/// * `framebuffer`: the framebuffer to draw in.
/// * `(start_x, start_y)`: the left top point of the retangle.
/// * `width`: the width of the rectangle.
/// * `height`: the height of the rectangle.
/// * `color`: the color of the rectangle's border.
pub fn draw_rectangle(
    framebuffer: &mut dyn FrameBuffer,
    location: AbsoluteCoord,
    width: usize,
    height: usize,
    color: u32,
) {
    let (buffer_width, buffer_height) = framebuffer.get_size();
    let (start_x, start_y) = location.coordinate();
    let end_x_offset: usize = {
        if start_x + width < buffer_width {
            width - 1
        } else {
            buffer_width - start_x - 1
        }
    };
    let end_y_offset: usize = {
        if start_y + height < buffer_height {
            height - 1
        } else {
            buffer_height - start_y - 1
        }
    };

    // draw the four lines of the rectangle.
    let mut top = location; 
    loop {
        if top.0.x == start_x + end_x_offset + 1 {
            break;
        }
        framebuffer.draw_pixel(top, color);
        framebuffer.draw_pixel(top + (0, end_y_offset), color);
        top = top + (1, 0);
    }

    let mut left = location; 
    loop {
        if left.0.y == start_y + end_y_offset + 1 {
            break;
        }
        framebuffer.draw_pixel(left, color);
        framebuffer.draw_pixel(left + (end_x_offset, 0), color);
        left = left + (0, 1);
    }
}

/// Fills a rectangle in a framebuffer with color.
/// The part exceeding the boundary of the framebuffer will be ignored.
/// # Arguments
/// * `framebuffer`: the framebuffer to draw in.
/// * `location`: the left top coordinate of the retangle.
/// * `width`: the width of the rectangle.
/// * `height`: the height of the rectangle.
/// * `color`: the color of the rectangle.
pub fn fill_rectangle(
    framebuffer: &mut dyn FrameBuffer,
    location: AbsoluteCoord,
    width: usize,
    height: usize,
    color: u32,
) {
    let (buffer_width, buffer_height) = framebuffer.get_size();
    let (start_x, start_y) = location.coordinate();

    let end_x: usize = {
        if start_x + width < buffer_width {
            start_x + width
        } else {
            buffer_width
        }
    };
    let end_y: usize = {
        if start_y + height < buffer_height {
            start_y + height
        } else {
            buffer_height
        }
    };

    // draw every pixel line by line
    let mut location = AbsoluteCoord::new(start_x, start_y);
    loop {
        loop {
            framebuffer.draw_pixel(location, color);
            location = location + (1, 0);
            if location.0.x == end_x {
                break;
            }
        }
        location = location + (0, 1);
        if location.0.y == end_y {
            break;
        }
        location.0.x = start_x;
    }
}
