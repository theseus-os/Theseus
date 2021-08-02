//! This crate defines a `Framebuffer` structure, which is effectively a region of memory
//! that is interpreted as a 2-D array of pixels.

#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate memory;
extern crate multicore_bringup;
extern crate owning_ref;
extern crate shapes;
extern crate color;
extern crate zerocopy;

pub mod pixel;
use alloc::boxed::Box;
use core::ops::DerefMut;

use memory::{EntryFlags, MappedPages, PhysicalAddress};
use owning_ref::BoxRefMut;
use shapes::Coord;
pub use pixel::*;

/// Initializes the final framebuffer based on VESA graphics mode information obtained during boot.
/// 
/// The final framebuffer represents the actual pixel content displayed on screen 
/// because its memory is directly mapped to the VESA display device's underlying physical memory.
pub fn init<P: Pixel>() -> Result<Framebuffer<P>, &'static str> {
    // get the graphic mode information
    let vesa_display_phys_start: PhysicalAddress;
    let buffer_width: usize;
    let buffer_height: usize;
    {
        let graphic_info = multicore_bringup::GRAPHIC_INFO.lock();
        info!("Using graphical framebuffer, {} x {}, at paddr {:#X}", graphic_info.width, graphic_info.height, graphic_info.physical_address);
        if graphic_info.physical_address == 0 {
            return Err("Failed to get graphic mode information!");
        }
        vesa_display_phys_start = PhysicalAddress::new(graphic_info.physical_address as usize)
            .ok_or("Graphic mode physical address was invalid")?;
        buffer_width = graphic_info.width as usize;
        buffer_height = graphic_info.height as usize;
    };

    // create and return the final framebuffer
    let framebuffer = Framebuffer::new(buffer_width, buffer_height, Some(vesa_display_phys_start))?;
    Ok(framebuffer)
}

/// A framebuffer is a region of memory interpreted as a 2-D array of pixels.
/// The memory buffer is a rectangular region with a width and height.
#[derive(Hash)]
pub struct Framebuffer<P: Pixel> {
    width: usize,
    height: usize,
    buffer: BoxRefMut<MappedPages, [P]>,
} 

impl<P: Pixel> Framebuffer<P> {
    /// Creates a new framebuffer with rectangular dimensions of `width * height`, 
    /// specified in number of pixels.
    /// If the `physical_address` is provided, the returned framebuffer will be **final**,
    /// i.e., mapped to the physical memory at that address, which is typically a hardware graphics device's memory.
    /// If the `physical_address` is `None`, the returned framebuffer is a "virtual" one 
    /// that renders to a randomly-allocated chunk of memory.
    pub fn new(
        width: usize,
        height: usize,
        physical_address: Option<PhysicalAddress>,
    ) -> Result<Framebuffer<P>, &'static str> {
        // get a reference to the kernel's memory mapping information
        let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;

        let vesa_display_flags: EntryFlags =
            EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;

        let size = width * height * core::mem::size_of::<P>();
        let pages = memory::allocate_pages_by_bytes(size).ok_or("could not allocate pages for a new framebuffer")?;

        let mapped_framebuffer = if let Some(address) = physical_address {
            let frames = memory::allocate_frames_by_bytes_at(address, size)
                .map_err(|_e| "Couldn't allocate frames for the final framebuffer")?;
            kernel_mmi_ref.lock().page_table.map_allocated_pages_to(
                pages,
                frames,
                vesa_display_flags,
            )?
        } else {
            kernel_mmi_ref.lock().page_table.map_allocated_pages(
                pages,
                vesa_display_flags,
            )?
        };

        // obtain a slice reference to the framebuffer's memory
        let buffer = BoxRefMut::new(Box::new(mapped_framebuffer))
            .try_map_mut(|mp| mp.as_slice_mut(0, width * height))?;

        Ok(Framebuffer {
            width: width,
            height: height,
            buffer: buffer,
        })
    }

    /// Returns a mutable reference to the mapped memory of the buffer.
    pub fn buffer_mut(&mut self) -> &mut BoxRefMut<MappedPages, [P]> {
        &mut self.buffer
    }

    /// Returns a reference to the mapped memory of the buffer
    pub fn buffer(&self) -> &BoxRefMut<MappedPages, [P]> {
        &self.buffer
    }

    /// Returns the `(width, height)` of this framebuffer.
    pub fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Composites `src` to the buffer starting from `index`.
    pub fn composite_buffer(&mut self, src: &[P], index: usize) {
        let len = src.len();
        let dest_end = index + len;
        Pixel::composite_buffer(src, &mut self.buffer_mut()[index..dest_end]);
    }

    /// Draw a pixel at the given 1coordinate1. 
    /// The `pixel` will be blended with the existing pixel value
    /// at that `coordinate` in this framebuffer.
    pub fn draw_pixel(&mut self, coordinate: Coord, pixel: P) {
        if let Some(index) = self.index_of(coordinate) {
            self.buffer[index] = pixel.blend(self.buffer[index]).into();
        }
    }

    /// Overwites a pixel at the given coordinate in this framebuffer
    /// instead of blending it like [`draw_pixel`](#method.draw_pixel).
    pub fn overwrite_pixel(&mut self, coordinate: Coord, pixel: P) {
        self.draw_pixel(coordinate, pixel)
    }

    /// Returns the pixel value at the given `coordinate` in this framebuffer.
    pub fn get_pixel(&self, coordinate: Coord) -> Option<P> {
        self.index_of(coordinate).map(|i| self.buffer[i])
    }

    /// Fills (overwrites) the entire framebuffer with the given `pixel` value.
    pub fn fill(&mut self, pixel: P) {
        for p in self.buffer.deref_mut() {
            *p = pixel;
        }
    }

    /// Returns the index of the given `coordinate` in this framebuffer,
    /// if this framebuffer [`contains`](#method.contains) the `coordinate` within its bounds.
    pub fn index_of(&self, coordinate: Coord) -> Option<usize> {
        if self.contains(coordinate) {
            Some((self.width * coordinate.y as usize) + coordinate.x as usize)
        } else {
            None
        }
    }

    /// Checks if the given `coordinate` is within the framebuffer's bounds.
    /// The `coordinate` is relative to the origin coordinate of `(0, 0)` being the top-left point of the framebuffer.
    pub fn contains(&self, coordinate: Coord) -> bool {
        coordinate.x >= 0
            && coordinate.x < (self.width as isize)
            && coordinate.y >= 0
            && coordinate.y < (self.height as isize)
    }

    /// Checks if a framebuffer overlaps with an area.
    /// # Arguments
    /// * `coordinate`: the top-left corner of the area relative to the origin(top-left point) of the framebuffer.
    /// * `width`: the width of the area in number of pixels.
    /// * `height`: the height of the area in number of pixels.
    pub fn overlaps_with(&mut self, coordinate: Coord, width: usize, height: usize) -> bool {
        coordinate.x < self.width as isize
            && coordinate.x + width as isize >= 0
            && coordinate.y < self.height as isize
            && coordinate.y + height as isize >= 0
    }

}
