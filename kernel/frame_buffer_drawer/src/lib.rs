//! This crate contains a series of basic draw functions to draw onto a framebuffer.
//! Displayables invoke these basic functions to display themselves onto a framebuffer.

#![no_std]

extern crate frame_buffer;

use frame_buffer::{FrameBuffer, AbsoluteCoord, ICoord};

/// Draws a point in a framebuffer.
/// The point is drawn at the coordinate of the framebuffer with color.
pub fn draw_point(framebuffer: &mut dyn FrameBuffer, coordinate: AbsoluteCoord, color: u32) {
    if framebuffer.contains_coordinate(coordinate) {
        framebuffer.draw_pixel(coordinate, color);
    }
}

/// Draws a line in a framebuffer. The part exceeding the boundary of the framebuffer will be ignored.
/// # Arguments
/// * `framebuffer`: the framebuffer to draw in.
/// * `start`: the start coordinate of the line.
/// * `end`: the end coordinate of the line.
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
            let coordinate = AbsoluteCoord::new(x as usize, y as usize);
            if framebuffer.contains_coordinate(coordinate) {
                framebuffer.draw_pixel(coordinate, color);
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
            let coordinate = AbsoluteCoord::new(x as usize, y as usize);
            if { framebuffer.contains_coordinate(coordinate) } {
                framebuffer.draw_pixel(coordinate, color);
            }
            y += step;
        }
    }
}

/// Draws a rectangle in a framebuffer.
/// The part exceeding the boundary of the framebuffer will be ignored.
/// # Arguments
/// * `framebuffer`: the framebuffer to draw in.
/// * `coordinate`: the left top coordinate of the retangle.
/// * `width`: the width of the rectangle.
/// * `height`: the height of the rectangle.
/// * `color`: the color of the rectangle's border.
pub fn draw_rectangle(
    framebuffer: &mut dyn FrameBuffer,
    coordinate: AbsoluteCoord,
    width: usize,
    height: usize,
    color: u32,
) {
    let (buffer_width, buffer_height) = framebuffer.get_size();
    let (start_x, start_y) = coordinate.value();
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
    let mut top = coordinate; 
    loop {
        if top.0.x == start_x + end_x_offset + 1 {
            break;
        }
        framebuffer.draw_pixel(top, color);
        framebuffer.draw_pixel(top + (0, end_y_offset), color);
        top = top + (1, 0);
    }

    let mut left = coordinate; 
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
/// * `coordinate`: the left top coordinate of the retangle.
/// * `width`: the width of the rectangle.
/// * `height`: the height of the rectangle.
/// * `color`: the color of the rectangle.
pub fn fill_rectangle(
    framebuffer: &mut dyn FrameBuffer,
    coordinate: AbsoluteCoord,
    width: usize,
    height: usize,
    color: u32,
) {
    let (buffer_width, buffer_height) = framebuffer.get_size();
    let (start_x, start_y) = coordinate.value();

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
    let mut coordinate = AbsoluteCoord::new(start_x, start_y);
    loop {
        loop {
            framebuffer.draw_pixel(coordinate, color);
            coordinate = coordinate + (1, 0);
            if coordinate.0.x == end_x {
                break;
            }
        }
        coordinate = coordinate + (0, 1);
        if coordinate.0.y == end_y {
            break;
        }
        coordinate.0.x = start_x;
    }
}
