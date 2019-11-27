//! This crate defines a `FrameBufferRGB` structure.
//! The structure implements the `FrameBuffer` trait. Every pixel in it is a RGB pixel without alpha channel.

#![no_std]

#[macro_use] extern crate alloc;
extern crate frame_buffer;
extern crate memory;
extern crate multicore_bringup;
extern crate owning_ref;
extern crate spin;

use alloc::boxed::Box;
use core::ops::DerefMut;
use frame_buffer::{FrameBuffer, Pixel, FINAL_FRAME_BUFFER, Coord};
use memory::{EntryFlags, FrameRange, MappedPages, PhysicalAddress, FRAME_ALLOCATOR};
use owning_ref::BoxRefMut;
use spin::Mutex;

// Every pixel is of u32 type
const PIXEL_BYTES: usize = 4;
const RGB_PIXEL_MASK: Pixel = 0x00FFFFFF;

/// Initialize the final frame buffer.
/// Allocates a block of memory and map it to the physical framebuffer frames.
pub fn init() -> Result<(), &'static str> {
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
    let framebuffer =
        FrameBufferRGB::new(buffer_width, buffer_height, Some(vesa_display_phys_start))?;
    let background = vec![0; buffer_width * buffer_height];
    FINAL_FRAME_BUFFER.call_once(|| Mutex::new(Box::new(framebuffer)));
    FINAL_FRAME_BUFFER.try().ok_or("").unwrap().lock().buffer_copy(background.as_slice(), 0);

    Ok(())
}

/// The RGB frame buffer structure. It implements the `FrameBuffer` trait.
#[derive(Hash)]
pub struct FrameBufferRGB {
    width: usize,
    height: usize,
    buffer: BoxRefMut<MappedPages, [Pixel]>,
}

impl FrameBufferRGB {
    /// Creates a new RGB frame buffer with specified size.
    /// If the `physical_address` is specified, the new virtual frame buffer will be mapped to hardware's physical memory at that address.
    /// If the `physical_address` is none, the new function will allocate a block of physical memory at a random address and map the new frame buffer to that memory.
    pub fn new(
        width: usize,
        height: usize,
        physical_address: Option<PhysicalAddress>,
    ) -> Result<FrameBufferRGB, &'static str> {
        // get a reference to the kernel's memory mapping information
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let allocator = FRAME_ALLOCATOR
            .try()
            .ok_or("Couldn't get Frame Allocator")?;

        let vesa_display_flags: EntryFlags =
            EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;

        let size = width * height * PIXEL_BYTES;
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

        Ok(FrameBufferRGB {
            width: width,
            height: height,
            buffer: buffer,
        })
    }

    /// Returns a mutable reference to the mapped memory of the buffer.
    pub fn buffer_mut(&mut self) -> &mut BoxRefMut<MappedPages, [Pixel]> {
        return &mut self.buffer;
    }
}

impl FrameBuffer for FrameBufferRGB {
    fn buffer(&self) -> &BoxRefMut<MappedPages, [Pixel]> {
        return &self.buffer;
    }

    fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn buffer_copy(&mut self, src: &[Pixel], dest_start: usize) {
        let len = src.len();
        let dest_end = dest_start + len;
        self.buffer_mut()[dest_start..dest_end].copy_from_slice(src);
    }

    fn draw_pixel(&mut self, coordinate: Coord, color: Pixel) {
        if let Some(index) = self.index(coordinate) {
            self.buffer[index] = color & RGB_PIXEL_MASK;
        }
    }

    fn overwrite_pixel(&mut self, coordinate: Coord, color: Pixel) {
        self.draw_pixel(coordinate, color)
    }

    fn get_pixel(&self, coordinate: Coord) -> Result<Pixel, &'static str> {
        if let Some(index) = self.index(coordinate) {
            return Ok(self.buffer[index] & RGB_PIXEL_MASK);
        } else {
            return Err("No pixel");
        }
    }

    fn fill_color(&mut self, color: Pixel) {
        for y in 0..self.height {
            for x in 0..self.width {
                let coordinate = Coord::new(x as isize, y as isize);
                self.draw_pixel(coordinate, color);
            }
        }
    }
}

/// Create a new framebuffer. useful for generalization
pub fn new(        
    width: usize,
    height: usize,
    physical_address: Option<PhysicalAddress>,
) -> Result<Box<dyn FrameBuffer>, &'static str> {
    let framebuffer = FrameBufferRGB::new(width, height, physical_address)?;
    Ok(Box::new(framebuffer))
}
