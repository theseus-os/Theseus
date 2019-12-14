//! This crate defines the `FrameBuffer` trait and maintains the final framebuffer.
//! A `FrameBuffer` contains fundamental display interfaces including displaying a single pixel and copying a buffer of pixels.

#![no_std]

extern crate alloc;
extern crate memory;
extern crate owning_ref;
extern crate spin;
extern crate downcast_rs;

use downcast_rs::Downcast;
use alloc::boxed::Box;
use memory::MappedPages;
use owning_ref::BoxRefMut;
use spin::{Mutex, Once};
use core::ops::{Add, Sub};
use core::cmp::{Ord, Ordering};


// /// The `FrameBuffer` trait.
// pub trait FrameBuffer: Downcast {
//     /// Returns a reference to the mapped memory.
//     fn buffer(&self) -> &BoxRefMut<MappedPages, [Pixel]>;

//     /// Gets the size of the framebuffer. 
//     /// Returns (width, height).
//     fn get_size(&self) -> (usize, usize);

//     /// Copies a buffer of pixels to the framebuffer from index `dest_start`.
//     fn composite_buffer(&mut self, src: &[Pixel], dest_start: usize);

//     /// Get the pixel at `coordinate` relative to the top-left point of the framebuffer
//     fn get_pixel(&self, coordinate: Coord) -> Result<Pixel, &'static str>;

//     /// Fill the framebuffer with `color`
//     fn fill_color(&mut self, color: Pixel);

//     /// Computes the index of a coordinate in the buffer array.
//     /// Return `None` if the coordinate is not in the frame buffer.
//     fn index(&self, coordinate: Coord) -> Option<usize> {
//         let (width, _) = self.get_size();
//         if self.contains(coordinate) {
//             return Some(coordinate.y as usize * width + coordinate.x as usize);
//         } else {
//             return None;
//         }
//     }


    
//     /// Draws a pixel at the given `coordinate` relative to the origin(top-left point) of the frame buffer. The new pixel is a mix of original pixel and `color` according to the type of the framebuffer.
//     fn draw_pixel(&mut self, coordinate: Coord, color: Pixel);

//     /// Draws a pixel at the given `coordinate` within the frame buffer. The `coordinate` is relative to the origin(top-left point) of the frame buffer.  The new pixel will overwrite the previous one.
//     fn overwrite_pixel(&mut self, coordinate: Coord, color: Pixel);



/// The coordinate of a pixel.
/// In the display subsystem, the origin of an area is its top-left point.
#[derive(Clone, Copy, PartialEq, Debug, Hash)]
pub struct Coord {
    /// The x coordinate
    pub x: isize,
    /// The y coordinate
    pub y: isize,
}

impl Coord {
    /// Creates a new coordinate.
    pub fn new(x: isize, y: isize) -> Coord {
        Coord { x: x, y: y }
    }
}

impl Add<(isize, isize)> for Coord {
    type Output = Coord;

    fn add(self, rhs: (isize, isize)) -> Coord {
        Coord { x: self.x + rhs.0, y: self.y + rhs.1 }
    }
}

impl Sub<(isize, isize)> for Coord {
    type Output = Coord;

    fn sub(self, rhs: (isize, isize)) -> Coord {
        Coord { x: self.x - rhs.0, y: self.y - rhs.1 }
    }
}

impl Add<Coord> for Coord {
    type Output = Coord;

    fn add(self, rhs: Coord) -> Coord {
        Coord {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub<Coord> for Coord {
    type Output = Coord;

    fn sub(self, rhs: Coord) -> Coord {
        Coord {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Ord for Coord {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.y > other.y {
            return Ordering::Greater;
        } else if self.y < other.y {
            return Ordering::Less;
        } else {
            return self.x.cmp(&other.x);
        }

    }
}

impl PartialOrd for Coord {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Coord { }


/// a rectangle region
#[derive(Clone, Copy, PartialEq, Debug, Hash)]
pub struct Rectangle {
    /// The top-left point
    pub top_left: Coord,
    /// The bottom-right point
    pub bottom_right: Coord,
}

impl Add<Coord> for Rectangle {
    type Output = Rectangle;

    fn add(self, rhs: Coord) -> Rectangle {
        Rectangle {
            top_left: self.top_left + rhs,
            bottom_right: self.bottom_right + rhs,
        }
    }
}

impl Sub<Coord> for Rectangle {
    type Output = Rectangle;

    fn sub(self, rhs: Coord) -> Rectangle {
        Rectangle {
            top_left: self.top_left - rhs,
            bottom_right: self.bottom_right - rhs,
        }
    }
}