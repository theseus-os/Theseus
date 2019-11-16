//! This crate defines the `FrameBuffer` trait and maintains the final framebuffer.
//! A `Framebuffer` contains fundamental display interfaces including displaying a single pixel and copying a buffer of pixels.

#![no_std]

extern crate alloc;
extern crate memory;
extern crate owning_ref;
extern crate spin;
#[macro_use]
extern crate downcast_rs;

use alloc::boxed::Box;
use core::cmp::{Ord, Ordering};
use core::ops::{Add, Sub};
use downcast_rs::Downcast;
use memory::MappedPages;
use owning_ref::BoxRefMut;
use spin::{Mutex, Once};

/// A pixel on the screen is mapped to a u32 integer.
pub type Pixel = u32;

/// The final framebuffer instance. It contains the pages which are mapped to the physical framebuffer.
pub static FINAL_FRAME_BUFFER: Once<Mutex<Box<dyn FrameBuffer + Send>>> = Once::new();

/// The `FrameBuffer` trait.
pub trait FrameBuffer: Downcast + Send {
    /// Returns a reference to the mapped memory.
    fn buffer(&self) -> &BoxRefMut<MappedPages, [Pixel]>;

    /// Gets the size of the framebuffer.
    /// Returns (width, height).
    fn get_size(&self) -> (usize, usize);

    /// Copies a buffer of pixels to the framebuffer from index `dest_start`.
    fn buffer_copy(&mut self, src: &[Pixel], dest_start: usize);

    /// Get the pixel at `coordinate`
    fn get_pixel(&self, coordinate: Coord) -> Result<Pixel, &'static str>;

    /// Fill the framebuffer with `color`
    fn fill_color(&mut self, color: Pixel);

    /// Computes the index of a coordinate in the buffer array.
    /// Return `None` if the coordinate is not in the frame buffer.
    fn index(&self, coordinate: Coord) -> Option<usize> {
        let (width, _) = self.get_size();
        if self.contains(coordinate) {
            return Some(coordinate.y as usize * width + coordinate.x as usize);
        } else {
            return None;
        }
    }

    /// Checks if a coordinate is within the framebuffer.
    fn contains(&self, coordinate: Coord) -> bool {
        let (width, height) = self.get_size();
        coordinate.x >= 0
            && coordinate.x < width as isize
            && coordinate.y >= 0
            && coordinate.y < height as isize
    }

    /// Draws a pixel at the given `coordinate` within the frame buffer. The `coordinate` is relative to the origin(top-left point) of the frame buffer.  The new pixel will overwrite the previous one.
    fn overwrite_pixel(&mut self, coordinate: Coord, color: Pixel);

    /// Checks if a framebuffer overlaps with an area.
    /// # Arguments
    /// * `coordinate`: the top-left corner of the area relative to the origin(top-left point) of the frame buffer.
    /// * `width`: the width of the area.
    /// * `height`: the height of the area.
    fn overlaps_with(&mut self, coordinate: Coord, width: usize, height: usize) -> bool {
        let (buffer_width, buffer_height) = self.get_size();
        coordinate.x < buffer_width as isize
            && coordinate.x + width as isize >= 0
            && coordinate.y < buffer_height as isize
            && coordinate.y + height as isize >= 0
    }

    /// Draws a pixel at the given `coordinate` relative to the origin(top-left point) of the frame buffer. The new pixel is a mix of original pixel and `color` according to the type of the framebuffer.
    fn draw_pixel(&mut self, coordinate: Coord, color: Pixel);

    /// Draw a circles in the framebuffer. `coordinate` is the position of the center of the circle, and `r` is the radius
    fn draw_circle(&mut self, center: Coord, r: usize, color: Pixel) {
        let r2 = (r * r) as isize;
        for y in center.y - r as isize..center.y + r as isize {
            for x in center.x - r as isize..center.x + r as isize {
                let coordinate = Coord::new(x, y);
                if self.contains(coordinate) {
                    let d = coordinate - center;
                    if d.x * d.x + d.y * d.y <= r2 {
                        self.draw_pixel(coordinate, color);
                    }
                }
            }
        }
    }

    /// Draws a rectangle on this framebuffer.
    fn draw_rect(&mut self, start: Coord, end: Coord, color: Pixel) {
        for y in start.y..end.y {
            for x in start.x..end.x {
                let coordinate = Coord::new(x, y);
                self.draw_pixel(coordinate, color);
            }
        }
    }
}
impl_downcast!(FrameBuffer);

/// Gets the size of the final framebuffer.
/// Returns (width, height).
pub fn get_screen_size() -> Result<(usize, usize), &'static str> {
    let final_buffer = FINAL_FRAME_BUFFER
        .try()
        .ok_or("The final frame buffer was not yet initialized")?
        .lock();
    Ok(final_buffer.get_size())
}

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
        Coord {
            x: self.x + rhs.0,
            y: self.y + rhs.1,
        }
    }
}

impl Sub<(isize, isize)> for Coord {
    type Output = Coord;

    fn sub(self, rhs: (isize, isize)) -> Coord {
        Coord {
            x: self.x - rhs.0,
            y: self.y - rhs.1,
        }
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

impl Eq for Coord {}
