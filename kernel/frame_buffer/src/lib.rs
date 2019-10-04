//! This crate defines the `FrameBuffer` trait and maintains the final framebuffer.
//! A `Framebuffer` contains fundamental display interfaces including displaying a single pixel and copying a buffer of pixels.

#![no_std]

extern crate alloc;
extern crate memory;
extern crate owning_ref;
extern crate spin;

use alloc::boxed::Box;
use memory::MappedPages;
use owning_ref::BoxRefMut;
use spin::{Mutex, Once};
use core::ops::Add;

/// A pixel on the screen is mapped to a u32 integer.
pub type Pixel = u32;

/// The final framebuffer instance. It contains the pages which are mapped to the physical framebuffer.
pub static FINAL_FRAME_BUFFER: Once<Mutex<Box<dyn FrameBuffer>>> = Once::new();

/// The `FrameBuffer` trait.
pub trait FrameBuffer: Send {
    /// Returns a reference to the mapped memory.
    fn buffer(&self) -> &BoxRefMut<MappedPages, [Pixel]>;

    /// Gets the size of the framebuffer. 
    /// Returns (width, height).
    fn get_size(&self) -> (usize, usize);

    /// Copies a buffer of pixels to the framebuffer from index `dest_start`.
    fn buffer_copy(&mut self, src: &[Pixel], dest_start: usize);

    /// Computes the index of a coordinate in the buffer array.
    fn index(&self, coordinate: Coord) -> Option<usize> {
        let (width, _) = self.get_size();
        if self.contains(coordinate) {
            return Some(coordinate.y as usize * width + coordinate.x as usize);
        } else {
            return None;
        }
    }

    /// Checks if a coordinate is within the framebuffer.
    fn contains(&self, coordinate: Coord) -> bool{
        let (width, height) = self.get_size();
        coordinate.x >= 0 && coordinate.x < width as isize 
            && coordinate.y >= 0 && coordinate.y < height as isize
    }

    /// Gets the indentical hash of the framebuffer.
    /// The frame buffer compositor uses this hash to cache framebuffers.
    fn get_hash(&self) -> u64;

    /// Draws a pixel at the given `coordinate` within the frame buffer. The `coordinate` is relative to the origin(top-left point) of the frame buffer.
    fn draw_pixel(&mut self, coordinate: Coord, color: Pixel);

    /// Checks if a framebuffer overlaps with an area.
    /// # Arguments
    /// * `coordinate`: the top-left corner of the area relative to the origin(top-left point) of the frame buffer.
    /// * `width`: the width of the area.
    /// * `height`: the height of the area.
    fn overlaps_with(&mut self, coordinate: Coord, width: usize, height: usize) -> bool {
        let (buffer_width, buffer_height) = self.get_size();
        coordinate.x < buffer_width as isize && coordinate.x + width as isize >= 0
            && coordinate.y < buffer_height as isize && coordinate.y + height as isize>= 0
    }
}

/// Gets the size of the final framebuffer.
/// Returns (width, height).
pub fn get_screen_size() -> Result<(usize, usize), &'static str> {
    let final_buffer = FINAL_FRAME_BUFFER
        .try()
        .ok_or("The final frame buffer was not yet initialized")?
        .lock();
    Ok(final_buffer.get_size())
}

/// The coordinate of a point.
/// In the display subsystem, the coordinate of an area represents the location of its top-left corner.
#[derive(Clone, Copy, PartialEq, Debug)]
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