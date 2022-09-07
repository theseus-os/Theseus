//! This crate contains a series of basic draw functions to draw onto a framebuffer.
//! Displayables invoke these basic functions to display themselves onto a framebuffer.
//! The coordinate in these interfaces is relative to the origin(top-left point) of the framebuffer.

#![no_std]

extern crate framebuffer;
extern crate shapes;

use framebuffer::{Framebuffer, Pixel};
use shapes::Coord;

/// Draws a line in a framebuffer. The part exceeding the boundary of the framebuffer will be ignored.
/// # Arguments
/// * `framebuffer`: the framebuffer to draw in.
/// * `start`: the start coordinate of the line relative to the origin(top-left point) of the framebuffer.
/// * `end`: the end coordinate of the line relative to the origin(top-left point) of the framebuffer.
/// * `color`: the color of the line.
pub fn draw_line<P: Pixel>(
    framebuffer: &mut Framebuffer<P>,
    start: Coord,
    end: Coord,
    pixel: P,
) {
    let width: isize = end.x - start.x;
    let height: isize = end.y - start.y;

    let mut line_in_buffer = false;
    
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
            let coordinate = Coord::new(x, y);
            if framebuffer.contains(coordinate) {
                line_in_buffer = true;
                framebuffer.draw_pixel(coordinate, pixel);
            } else if line_in_buffer {
                // the part exceeds the buffer will be ignored
                break;
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
            let coordinate = Coord::new(x, y);
            if framebuffer.contains(coordinate) {
                line_in_buffer = true;
                framebuffer.draw_pixel(coordinate, pixel);
            } else if line_in_buffer {
                // the part exceeds the buffer will be ignored
                break;
            }
            y += step;
        }
    }
}

/// Draws a rectangle in a framebuffer.
/// The part exceeding the boundary of the framebuffer will be ignored.
/// # Arguments
/// * `framebuffer`: the framebuffer to draw in.
/// * `coordinate`: the left top coordinate of the rectangle relative to the origin(top-left point) of the framebuffer.
/// * `width`: the width of the rectangle in number of pixels.
/// * `height`: the height of the rectangle in number of pixels.
/// * `color`: the color of the rectangle's border.
pub fn draw_rectangle<P: Pixel>(
    framebuffer: &mut Framebuffer<P>,
    coordinate: Coord,
    width: usize,
    height: usize,
    pixel: P,
) {
    let (buffer_width, buffer_height) = framebuffer.get_size();

    // return if the rectangle is not within the framebuffer
    if !framebuffer.overlaps_with(coordinate, width, height){
        return
    }

    // draw the part within the framebuffer
    let start_x = core::cmp::max(coordinate.x, 0);
    let start_y = core::cmp::max(coordinate.y, 0);
    let end_x = core::cmp::min(coordinate.x + width as isize, buffer_width as isize);
    let end_y = core::cmp::min(coordinate.y + height as isize, buffer_height as isize);

    // draw the four lines of the rectangle.
    let mut top = Coord::new(start_x, start_y);
    let end_y_offset = end_y - start_y - 1;
    loop {
        if top.x == end_x {
            break;
        }
        if coordinate.y >= 0 {
            framebuffer.draw_pixel(top, pixel);
        }
        if (coordinate.y + height as isize) < buffer_height as isize { 
            framebuffer.draw_pixel(top + (0, end_y_offset), pixel);
        }
        top.x += 1;
    }

    let mut left = Coord::new(start_x, start_y); 
    let end_x_offset = end_x - start_x - 1;
    loop {
        if left.y == end_y {
            break;
        }
        if coordinate.x >= 0 {
            framebuffer.draw_pixel(left, pixel);
        }
        if (coordinate.x + width as isize) < buffer_width as isize {
            framebuffer.draw_pixel(left + (end_x_offset, 0), pixel);
        }
        left.y += 1;
    }
}

/// Fills a rectangle in a framebuffer with color.
/// The part exceeding the boundary of the framebuffer will be ignored.
/// # Arguments
/// * `framebuffer`: the framebuffer to draw in.
/// * `coordinate`: the left top coordinate of the retangle relative to the origin(top-left point) of the framebuffer.
/// * `width`: the width of the rectangle in number of pixels.
/// * `height`: the height of the rectangle in number of pixels.
/// * `pixel`: the value of pixels in the rectangle.
pub fn fill_rectangle<P: Pixel>(
    framebuffer: &mut Framebuffer<P>,
    coordinate: Coord,
    width: usize,
    height: usize,
    pixel: P,
) {
    let (buffer_width, buffer_height) = framebuffer.get_size();
    // return if the rectangle is not within the framebuffer
    if !framebuffer.overlaps_with(coordinate, width, height){
        return
    }

    // draw the part within the framebuffer
    let start_x = core::cmp::max(coordinate.x, 0);
    let start_y = core::cmp::max(coordinate.y, 0);
    let end_x = core::cmp::min(coordinate.x + width as isize, buffer_width as isize);
    let end_y = core::cmp::min(coordinate.y + height as isize, buffer_height as isize);

    // draw every pixel line by line
    let mut coordinate = Coord::new(start_x, start_y);
    loop {
        loop {
            framebuffer.draw_pixel(coordinate, pixel);
            coordinate.x += 1;
            if coordinate.x == end_x {
                break;
            }
        }
        coordinate.y += 1;
        if coordinate.y == end_y {
            break;
        }
        coordinate.x = start_x;
    }
}

/// Draw a circle in the framebuffer. `coordinate` is the position of the center of the circle relative to the top-left corner of the framebuffer and `r` is the radius
pub fn draw_circle<P: Pixel>(framebuffer: &mut Framebuffer<P>, center: Coord, r: usize, pixel: P) {
    let r2 = (r * r) as isize;
    for y in center.y - r as isize..center.y + r as isize {
        for x in center.x - r as isize..center.x + r as isize {
            let coordinate = Coord::new(x, y);
            if framebuffer.contains(coordinate) {
                let d = coordinate - center;
                if d.x * d.x + d.y * d.y <= r2 {
                    framebuffer.draw_pixel(coordinate, pixel);
                }
            }
        }
    }
}