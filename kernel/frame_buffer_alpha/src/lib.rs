//! Framebuffer with alpha channel composition
//!
//! It defines a `FrameBufferAlpha` structure and creates new framebuffers for applications.
//! It also defines the color format which is composed of blue, green, red and alpha.
//!
//! Several predefined functions can help to manipulate the framebuffer objects and single pixel.

#![no_std]

#[macro_use]
extern crate log;

extern crate alloc;
extern crate font;
extern crate frame_buffer;
extern crate memory;
extern crate multicore_bringup;
extern crate owning_ref;
extern crate spin;

use alloc::boxed::Box;
use core::ops::DerefMut;
use frame_buffer::{Coord, FrameBuffer, Pixel, FINAL_FRAME_BUFFER};
use memory::{EntryFlags, FrameRange, MappedPages, PhysicalAddress, FRAME_ALLOCATOR};
use owning_ref::BoxRefMut;
use spin::Mutex;

/// predefined opaque black
pub const BLACK: Pixel = 0;
/// predefined opaque white
pub const WHITE: Pixel = 0x00FFFFFF;

/// Every pixel is of `Pixel` type, which is 4 byte as defined in `Pixel`
const PIXEL_BYTES: usize = core::mem::size_of::<Pixel>();

/// Initialize the final frame buffer by allocating a block of memory and map it to the physical framebuffer frames.
pub fn init() -> Result<(usize, usize), &'static str> {
    // Get the graphic mode information
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

    debug!(
        "frame_buffer_alpha init with width({}) height({})",
        buffer_width, buffer_height
    );
    // init the final framebuffer
    let framebuffer =
        FrameBufferAlpha::new(buffer_width, buffer_height, Some(vesa_display_phys_start))?;
    FINAL_FRAME_BUFFER.call_once(|| Mutex::new(Box::new(framebuffer)));

    Ok((buffer_width, buffer_height))
}

/// The frame buffer struct of either virtual frame buffer or physical frame buffer. It contains the size of the buffer and a buffer array. Pixels in this buffer have alpha channel.
pub struct FrameBufferAlpha {
    /// The width of the framebuffer
    pub width: usize,
    /// The height of the framebuffer
    pub height: usize,
    /// The memory buffer
    pub buffer: BoxRefMut<MappedPages, [Pixel]>,
}

impl FrameBufferAlpha {
    /// Create a new frame buffer with specified size.
    /// If the physical_address is specified, the new frame buffer will be mapped to hardware's physical memory at that address.
    /// If the physical_address is none, the new function will allocate a block of physical memory at a random address and map the new frame buffer to that memory, which is a virtual frame buffer.
    pub fn new(
        width: usize,
        height: usize,
        physical_address: Option<PhysicalAddress>,
    ) -> Result<FrameBufferAlpha, &'static str> {
        // get a reference to the kernel's memory mapping information
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let allocator = FRAME_ALLOCATOR
            .try()
            .ok_or("Couldn't get Frame Allocator")?;

        let size = width * height * PIXEL_BYTES;
        let pages = memory::allocate_pages_by_bytes(size).ok_or("could not allocate pages")?;

        let mapped_frame_buffer = if let Some(address) = physical_address {
            let frame = FrameRange::from_phys_addr(address, size);
            kernel_mmi_ref.lock().page_table.map_allocated_pages_to(
                pages,
                frame,
                EntryFlags::PRESENT
                    | EntryFlags::WRITABLE
                    | EntryFlags::GLOBAL
                    | EntryFlags::NO_CACHE,
                allocator.lock().deref_mut(),
            )?
        } else {
            kernel_mmi_ref.lock().page_table.map_allocated_pages(
                pages,
                EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL, // | EntryFlags::NO_CACHE,
                allocator.lock().deref_mut(),
            )?
        };

        // create a refence to transmute the mapped frame buffer pages as a slice
        let buffer = BoxRefMut::new(Box::new(mapped_frame_buffer))
            .try_map_mut(|mp| mp.as_slice_mut(0, width * height))?;

        Ok(FrameBufferAlpha {
            width: width,
            height: height,
            buffer: buffer,
        })
    }

    /// return a mutable reference to the buffer
    pub fn buffer_mut(&mut self) -> &mut BoxRefMut<MappedPages, [Pixel]> {
        return &mut self.buffer;
    }

    /// return a reference to the buffer
    pub fn buffer(&self) -> &BoxRefMut<MappedPages, [Pixel]> {
        return &self.buffer;
    }

    /// get the size of the frame buffer. Return (width, height).
    pub fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}

impl FrameBuffer for FrameBufferAlpha {
    fn buffer(&self) -> &BoxRefMut<MappedPages, [Pixel]> {
        return &self.buffer;
    }

    fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn buffer_copy(&mut self, src: &[Pixel], dest_start: usize) {
        let len = src.len();
        for i in 0..len {
            let index = dest_start + i;
            let coordinate =
                Coord::new((index % self.width) as isize, (index / self.width) as isize);
            self.draw_pixel(coordinate, src[i]);
        }
    }

    fn overwrite_pixel(&mut self, coordinate: Coord, color: Pixel) {
        if let Some(idx) = self.index(coordinate) {
            self.buffer[idx] = color;
        };
    }

    fn draw_pixel(&mut self, coordinate: Coord, color: Pixel) {
        let idx = match self.index(coordinate) {
            Some(index) => index,
            None => return,
        };
        let origin = Pixel::from(self.buffer[idx]);
        self.buffer[idx] = Pixel::from(color).alpha_mix(origin);
    }

    fn get_pixel(&self, coordinate: Coord) -> Result<Pixel, &'static str> {
        let idx = match self.index(coordinate) {
            Some(index) => index,
            None => {
                return Err("get pixel out of bound");
            }
        };
        Ok(Pixel::from(self.buffer[idx]))
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

/// A pixel Mixer provides methods to mix two pixels
pub trait PixelMixer {
    /// mix two color using alpha channel composition, supposing `self` is on the top of `other` pixel.
    fn alpha_mix(self, other: Self) -> Self;

    /// mix two color linearly with weights, as `mix` for `self` and (1-`mix`) for `other`. It returns black if mix is outside range of [0, 1].
    fn color_mix(self, other: Self, mix: f32) -> Self;

    /// Gets the alpha channel of the pixel
    fn get_alpha(&self) -> u8;

    /// Gets the red byte of the pixel
    fn get_red(&self) -> u8;

    /// Gets the green byte of the pixel
    fn get_green(&self) -> u8;

    /// Gets the blue byte of the pixel
    fn get_blue(&self) -> u8;
}

impl PixelMixer for Pixel {
    fn alpha_mix(self, other: Self) -> Self {
        let alpha = self.get_alpha() as u16;
        let red = self.get_red();
        let green = self.get_green();
        let blue = self.get_blue();
        // let ori_alpha = other.get_alpha();
        let ori_red = other.get_red();
        let ori_green = other.get_green();
        let ori_blue = other.get_blue();
        // let new_alpha = (((alpha as u16) * (255 - alpha) + (ori_alpha as u16) * alpha) / 255) as u8;
        let new_red = (((red as u16) * (255 - alpha) + (ori_red as u16) * alpha) / 255) as u8;
        let new_green = (((green as u16) * (255 - alpha) + (ori_green as u16) * alpha) / 255) as u8;
        let new_blue = (((blue as u16) * (255 - alpha) + (ori_blue as u16) * alpha) / 255) as u8;
        return new_alpha_pixel(alpha as u8, new_red, new_green, new_blue);
    }

    fn color_mix(self, other: Self, mix: f32) -> Self {
        if mix < 0f32 || mix > 1f32 {
            return BLACK;
        }
        let new_alpha =
            ((self.get_alpha() as f32) * mix + (other.get_alpha() as f32) * (1f32 - mix)) as u8;
        let new_red =
            ((self.get_red() as f32) * mix + (other.get_red() as f32) * (1f32 - mix)) as u8;
        let new_green =
            ((self.get_green() as f32) * mix + (other.get_green() as f32) * (1f32 - mix)) as u8;
        let new_blue =
            ((self.get_blue() as f32) * mix + (other.get_blue() as f32) * (1f32 - mix)) as u8;
        return new_alpha_pixel(new_alpha, new_red, new_green, new_blue);
    }

    fn get_alpha(&self) -> u8 {
        (self >> 24) as u8
    }

    fn get_red(&self) -> u8 {
        (self >> 16) as u8
    }

    fn get_green(&self) -> u8 {
        (self >> 8) as u8
    }

    fn get_blue(&self) -> u8 {
        self.clone() as u8
    }
}

/// Create a new Pixel from `alpha`, `red`, `green` and `blue` bytes.
pub fn new_alpha_pixel(alpha: u8, red: u8, green: u8, blue: u8) -> Pixel {
    return ((alpha as u32) << 24) + ((red as u32) << 16) + ((green as u32) << 8) + (blue as u32);
}

/// Create a new alpha framebuffer. useful for generalization.
pub fn new(
    width: usize,
    height: usize,
    physical_address: Option<PhysicalAddress>,
) -> Result<Box<dyn FrameBuffer>, &'static str> {
    let framebuffer = FrameBufferAlpha::new(width, height, physical_address)?;
    Ok(Box::new(framebuffer))
}
