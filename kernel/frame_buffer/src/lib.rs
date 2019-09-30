//! This crate defines the `FrameBuffer` trait and maintains the final framebuffer.
//! A `Framebuffer` contains fundamental display interfaces including displaying a single pixel and copying a buffer of pixels.

#![no_std]

extern crate alloc;
extern crate memory;
extern crate multicore_bringup;
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

    /// Computes the index of pixel (x, y) in the buffer array.
    fn index(&self, coordinate: AbsoluteCoord) -> usize;

    /// Checks if a pixel (x, y) is within the framebuffer.
    fn contains_coordinate(&self, coordinate: AbsoluteCoord) -> bool;

    /// Gets the indentical hash of the framebuffer.
    /// The frame buffer compositor uses this hash to cache framebuffers.
    fn get_hash(&self) -> u64;

    /// Draws a pixel in the framebuffer.
    fn draw_pixel(&mut self, coordinate: AbsoluteCoord, color: Pixel);
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
#[derive(Clone, Copy)]
pub struct UCoord {
    pub x: usize,
    pub y: usize,
}

/// The coordinate of a point.
#[derive(Clone, Copy, PartialEq)]
pub struct ICoord {
    pub x: i32,
    pub y: i32,
}

impl Add<(i32, i32)> for ICoord {
    type Output = ICoord;

    fn add(self, rhs: (i32, i32)) -> ICoord {
        ICoord { x: self.x + rhs.0, y: self.y + rhs.1 }
    }
}

/// The absolute coordinate of a point in a buffer. 
#[derive(Clone, Copy)]
pub struct AbsoluteCoord(pub UCoord);

impl AbsoluteCoord {
    /// Create an absolute coordinate.
    pub fn new(x: usize, y: usize) -> AbsoluteCoord {
        AbsoluteCoord(
            UCoord {
                x: x,
                y: y,
            }
        )
    }

    /// Get the (x, y) value of the coordinate.
    #[inline]
    pub fn value(&self) -> (usize, usize) {
        (self.0.x, self.0.y)
    } 
}

impl Add<(usize, usize)> for AbsoluteCoord {
    type Output = AbsoluteCoord;

    fn add(self, rhs: (usize, usize)) -> AbsoluteCoord {
        AbsoluteCoord::new(self.0.x + rhs.0, self.0.y + rhs.1)
    }
}


/// The coordinate of a point relative to some 
#[derive(Clone, Copy)]
pub struct RelativeCoord(UCoord);

impl RelativeCoord {
    pub fn new(x: usize, y: usize) -> RelativeCoord {
        RelativeCoord(
            UCoord {
                x: x,
                y: y,
            }
        )
    }

    #[inline]
    pub fn value(&self) -> (usize, usize) {
        (self.0.x, self.0.y)
    }

    #[inline]
    pub fn inner(&self) -> UCoord {
        self.0
    } 
}

impl Add<(usize, usize)> for RelativeCoord {
    type Output = RelativeCoord;

    fn add(self, rhs: (usize, usize)) -> RelativeCoord {
        RelativeCoord::new(self.0.x + rhs.0, self.0.y + rhs.1)
    }
}
