//! This crate defines the `FrameBuffer` trait and maintains the final framebuffer.
//! A `Framebuffer` contains fundamental display interfaces.

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

    /// Displays a buffer of pixels in the framebuffer from index `dest_start`.
    fn buffer_copy(&mut self, src: &[Pixel], dest_start: usize);

    /// Computes the index of pixel (x, y) in the buffer array.
    fn index(&self, x: usize, y: usize) -> usize;

    /// Checks if a pixel (x, y) is within the framebuffer.
    fn check_in_buffer(&self, x: usize, y: usize) -> bool;

    /// Gets the indentical hash of the framebuffer.
    /// The frame buffer compositor uses this hash to cache framebuffers.
    fn get_hash(&self) -> u64;

    /// Draws a pixel in the framebuffer.
    fn draw_pixel(&mut self, x: usize, y: usize, color: Pixel);
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
