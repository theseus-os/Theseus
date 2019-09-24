//! This crate is a frame buffer manager.
//! * It defines a FrameBuffer structure and creates new framebuffers for applications

#![no_std]

extern crate multicore_bringup;
extern crate spin;
extern crate alloc;
extern crate memory;
extern crate owning_ref;
#[macro_use] extern crate downcast_rs;

use alloc::boxed::Box;
use core::ops::DerefMut;
use memory::{EntryFlags, FrameRange, MappedPages,PhysicalAddress, FRAME_ALLOCATOR};
use owning_ref::BoxRefMut;
use spin::{Mutex, Once};
use downcast_rs::Downcast;

/// A Pixel is a u32 integer. The lower 24 bits of a Pixel specifie the RGB color of a pixel
pub type Pixel = u32;

/// The final framebuffer instance. It contains the pages which are mapped to the physical framebuffer
pub static FINAL_FRAME_BUFFER: Once<Mutex<Box<FrameBuffer>>> = Once::new();

// Every pixel is of u32 type
const PIXEL_BYTES: usize = 4;

pub trait FrameBuffer: Send {
    /// return a mutable reference to the buffer
    //fn buffer_mut(&mut self) -> &mut BoxRefMut<MappedPages, [Pixel]>;
    
    /// return a reference to the buffer
    fn buffer(&self) -> &BoxRefMut<MappedPages, [Pixel]>;

    /// get the size of the frame buffer. Return (width, height).
    fn get_size(&self) -> (usize, usize);

    fn buffer_copy(&mut self, src:&[Pixel], dest_start:usize);
    // ///get a function to compute the index of a pixel in the buffer array. The returned function is (x:usize, y:usize) -> index:usize
    // pub fn get_index_fn(&self) -> Box<Fn(usize, usize)->usize>{
    //     let width = self.width;
    //     Box::new(move |x:usize, y:usize| y * width + x )
    // }

    /// compute the index of pixel (x, y) in the buffer array
    fn index(&self, x: usize, y: usize) -> usize;

    /// check if a pixel (x,y) is within the framebuffer
    fn check_in_buffer(&self, x: usize, y: usize) -> bool;

    fn get_hash(&self) -> u64;

    /// write a pixel to a framebuffer directly
    fn draw_pixel(&mut self, x: usize, y: usize, color: Pixel);

}


/// Get the size of the final framebuffer. Return (width, height)
pub fn get_screen_size() -> Result<(usize, usize), &'static str> {
    let final_buffer = FINAL_FRAME_BUFFER
        .try()
        .ok_or("The final frame buffer was not yet initialized")?
        .lock();
    Ok(final_buffer.get_size())
}

