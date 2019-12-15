//! This crate defines a `FrameBuffer` structure.
//! The structure contains a buffer of pixels. A compositor can composite a list of framebuffers to a final one. 
//!
//! `pixel.rs` defines two type of pixels which implement the `Pixel` trait. The lower three bytes represent the color of a pixel and the 4th byte is an extra channel. `RGBPixel` is a normal pixel without channel and `AlphaPixel` has an alpha channel. The pixels to be mixed together should be of the same type in case the semantic of their channels are different.

#![no_std]

extern crate alloc;
extern crate memory;
extern crate multicore_bringup;
extern crate owning_ref;
extern crate shapes;

pub mod pixel;
use alloc::boxed::Box;
use core::ops::DerefMut;

use memory::{EntryFlags, FrameRange, MappedPages, PhysicalAddress, FRAME_ALLOCATOR};
use owning_ref::BoxRefMut;
use shapes::Coord;
use core::marker::PhantomData;
pub use pixel::*;

/// Initialize the final frame buffer and returns it.
/// The final framebuffer contains a block of memory which is mapped to the physical framebuffer frames.
pub fn init<T: Pixel>() -> Result<FrameBuffer<T>, &'static str> {
    // get the graphic mode information
    let vesa_display_phys_start: PhysicalAddress;
    let buffer_width: usize;
    let buffer_height: usize;
    {
        let graphic_info = multicore_bringup::GRAPHIC_INFO.lock();
        if graphic_info.physical_address == 0 {
            return Err("Fail to get graphic mode infomation!");
        }
        vesa_display_phys_start = PhysicalAddress::new(graphic_info.physical_address as usize)?;
        buffer_width = graphic_info.width as usize;
        buffer_height = graphic_info.height as usize;
    };

    // init the final framebuffer
    let framebuffer = FrameBuffer::new(buffer_width, buffer_height, Some(vesa_display_phys_start))?;

    Ok(framebuffer)
}

/// The RGB frame buffer structure. It implements the `FrameBuffer` trait.
#[derive(Hash)]
pub struct FrameBuffer<T: Pixel> {
    width: usize,
    height: usize,
    buffer: BoxRefMut<MappedPages, [T]>,
    _phantom: PhantomData<T>,
} 

impl<T: Pixel> FrameBuffer<T> {
    /// Creates a new RGB frame buffer with specified size.
    /// If the `physical_address` is specified, the new virtual frame buffer will be mapped to hardware's physical memory at that address.
    /// If the `physical_address` is none, the new function will allocate a block of physical memory at a random address and map the new frame buffer to that memory.
    pub fn new(
        width: usize,
        height: usize,
        physical_address: Option<PhysicalAddress>,
    ) -> Result<FrameBuffer<T>, &'static str> {
        // get a reference to the kernel's memory mapping information
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let allocator = FRAME_ALLOCATOR
            .try()
            .ok_or("Couldn't get Frame Allocator")?;

        let vesa_display_flags: EntryFlags =
            EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;

        let size = width * height * PIXEL_SIZE;
        let pages = memory::allocate_pages_by_bytes(size).ok_or("could not allocate pages")?;

        let mapped_frame_buffer = if let Some(address) = physical_address {
            let frame = FrameRange::from_phys_addr(address, size);
            kernel_mmi_ref.lock().page_table.map_allocated_pages_to(
                pages,
                frame,
                vesa_display_flags,
                allocator.lock().deref_mut()
            )?
        } else {
            kernel_mmi_ref.lock().page_table.map_allocated_pages(
                pages,
                vesa_display_flags,
                allocator.lock().deref_mut()
            )?
        };

        // create a refence to transmute the mapped frame buffer pages as a slice
        let buffer = BoxRefMut::new(Box::new(mapped_frame_buffer))
            .try_map_mut(|mp| mp.as_slice_mut(0, width * height))?;

        Ok(FrameBuffer {
            width: width,
            height: height,
            buffer: buffer,
            _phantom: PhantomData
        })
    }

    /// Returns a mutable reference to the mapped memory of the buffer.
    pub fn buffer_mut(&mut self) -> &mut BoxRefMut<MappedPages, [T]> {
        return &mut self.buffer;
    }

    /// Returns a referece to the mapped memory of the buffer
    pub fn buffer(&self) -> &BoxRefMut<MappedPages, [T]> {
        return &self.buffer;
    }

    /// Returns (width, height) of the framebuffer
    pub fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Composites `src` to the buffer starting from `index`.
    pub fn composite_buffer(&mut self, src: &[T], index: usize) {
        let len = src.len();
        let dest_end = index + len;
        T::composite_buffer(src, &mut self.buffer_mut()[index..dest_end]);
    }

    /// Draw a pixel at the given coordinate. The pixel will mix with the original one at the coordinate.
    pub fn draw_pixel(&mut self, coordinate: Coord, pixel: T) {
        if let Some(index) = self.index(coordinate) {
            self.buffer[index] = pixel.mix(self.buffer[index]).into();
        }
    }

    /// Overwites a pixel at the given coordinate.
    pub fn overwrite_pixel(&mut self, coordinate: Coord, color: T) {
        self.draw_pixel(coordinate, color)
    }

    /// Returns a pixel at coordinate.
    pub fn get_pixel(&self, coordinate: Coord) -> Result<T, &'static str> {
        if let Some(index) = self.index(coordinate) {
            return Ok(self.buffer[index]);
        } else {
            return Err("No pixel");
        }
    }

    /// Fills the framebuffer with color.
    pub fn fill_color(&mut self, color: T) {
        for y in 0..self.height {
            for x in 0..self.width {
                let coordinate = Coord::new(x as isize, y as isize);
                self.draw_pixel(coordinate, color);
            }
        }
    }

    /// Returns the index of the coordinate in the buffer
    pub fn index(&self, coordinate: Coord) -> Option<usize> {
        if self.contains(coordinate) {
            return Some(coordinate.y as usize * self.width + coordinate.x as usize);
        } else {
            return None;
        }
    }

    /// Checks if a coordinate is within the framebuffer.
    pub fn contains(&self, coordinate: Coord) -> bool {
        let (width, height) = self.get_size();
        coordinate.x >= 0 && coordinate.x < width as isize
            && coordinate.y >= 0 && coordinate.y < height as isize
    }

    /// Checks if a framebuffer overlaps with an area.
    /// # Arguments
    /// * `coordinate`: the top-left corner of the area relative to the origin(top-left point) of the frame buffer.
    /// * `width`: the width of the area.
    /// * `height`: the height of the area.
    pub fn overlaps_with(&mut self, coordinate: Coord, width: usize, height: usize) -> bool {
        let (buffer_width, buffer_height) = self.get_size();
        coordinate.x < buffer_width as isize && coordinate.x + width as isize >= 0
            && coordinate.y < buffer_height as isize && coordinate.y + height as isize >= 0
    }

}
