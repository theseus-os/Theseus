//! Framebuffer with alpha channel composition
//! 
//! It defines a FrameBufferAlpha structure and creates new framebuffers for applications, 
//! also defines the color format which is composed of blue, green, red and alpha.
//!
//! Several predefined functions can help to manipulate the framebuffer objects and single pixel.

#![no_std]

#[macro_use] extern crate log;

extern crate multicore_bringup;
extern crate spin;
extern crate alloc;
extern crate memory;
extern crate owning_ref;
extern crate font;
extern crate frame_buffer;

use alloc::boxed::Box;
use core::ops::DerefMut;
use memory::{EntryFlags, FrameRange, MappedPages,PhysicalAddress, FRAME_ALLOCATOR};
use owning_ref::BoxRefMut;
use frame_buffer::{FrameBuffer, Pixel, Coord, FINAL_FRAME_BUFFER};
use spin::Mutex;

/// A `Pixel` is a `u32` broken down into four bytes. 
/// The lower 24 bits of a Pixel specify its RGB color values, while the upper 8bit is an `alpha` channel,
/// in which an `alpha` value of `0` represents an opaque pixel and `0xFF` represents a completely transparent pixel. 
/// The `alpha` value is used in an alpha-blending compositor that supports transparency.
pub type AlphaPixel = u32;

/// predefined opaque black
pub const BLACK: AlphaPixel = 0;
/// predefined opaque white
pub const WHITE: AlphaPixel = 0x00FFFFFF;

// Every pixel is of `Pixel` type, which is 4 byte as defined in `Pixel`
const PIXEL_BYTES: usize = core::mem::size_of::<AlphaPixel>();

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

    debug!("frame_buffer_alpha init with width({}) height({})", buffer_width, buffer_height);
    // init the final framebuffer
    let framebuffer =
        FrameBufferAlpha::new(buffer_width, buffer_height, Some(vesa_display_phys_start))?;
    FINAL_FRAME_BUFFER.call_once(|| Mutex::new(Box::new(framebuffer)));

    Ok((buffer_width, buffer_height))
}

/// The frame buffer struct of either virtual frame buffer or physical frame buffer. It contains the size of the buffer and a buffer array
pub struct FrameBufferAlpha {
    pub width: usize,
    pub height: usize,
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
            try!(kernel_mmi_ref.lock().page_table.map_allocated_pages_to(
                pages,
                frame,
                EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE,
                allocator.lock().deref_mut()
            ))
        } else {
            try!(kernel_mmi_ref.lock().page_table.map_allocated_pages(
                pages,
                EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL, // | EntryFlags::NO_CACHE,
                allocator.lock().deref_mut()
            ))
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

    /// draw a rectangle on this framebuffer
    pub fn draw_rect(&mut self, xs: usize, xe: usize, ys: usize, ye: usize, color: AlphaPixel) {
        for y in ys..ye {
            for x in xs..xe {
                let coordinate = Coord::new(x as isize, y as isize);
                self.draw_pixel(coordinate, color);
            }
        }
    }

    /// draw a rectangle on this framebuffer with alpha
    pub fn draw_rect_alpha(&mut self, xs: usize, xe: usize, ys: usize, ye: usize, color: AlphaPixel) {
        // Wenqiu: TODO use coordinate
        for y in ys..ye {
            for x in xs..xe {
                let coordinate = Coord::new(x as isize, y as isize);
                self.draw_pixel_alpha(coordinate, color);
            }
        }
    }

    /// draw a char on the screen with alpha
    pub fn draw_char_8x16(&mut self, coordinate: Coord, c: u8, color: AlphaPixel) {
        for yi in 0..16 {
            let char_font: u8 = font::FONT_BASIC[c as usize][yi];
            for xi in 0..8 {
                const HIGHEST_BIT: u8 = 0x80;
                if char_font & (HIGHEST_BIT >> xi) != 0 {
                    self.draw_pixel_alpha(coordinate + (xi as isize, yi as isize), color);
                }
            }
        }
    }
}


impl FrameBuffer for FrameBufferAlpha {
    // Wenqiu: TODO move to trait
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
        if let Some(idx) = self.index(coordinate) {
            self.buffer[idx] = color;
        };
    }

    fn draw_pixel_alpha(&mut self, coordinate: Coord, color: AlphaPixel) {
        let idx = match self.index(coordinate) {
            Some(index) => { index },
            None => { return }
        };
        let origin = AlphaPixel::from(self.buffer[idx]);
        self.buffer[idx] = AlphaPixel::from(color).alpha_mix(origin);
    }

    /// get one pixel at given position
    fn get_pixel(& self, coordinate: Coord) -> Result<Pixel, &'static str> {
        let idx = match self.index(coordinate) {
            Some(index) => { index },
            None => { return Err("get pixel out of bound"); }
        };
        Ok(AlphaPixel::from(self.buffer[idx]))
    }

        /// fill the entire frame buffer with given `color`
    fn fill_color(&mut self, color: Pixel) {
        for y in 0..self.height {
            for x in 0..self.width {
                let coordinate = Coord::new(x as isize, y as isize);
                self.draw_pixel(coordinate, color);
            }
        }
    }
}

pub trait PixelMixer {
   /// mix two color using alpha channel composition, supposing `self` is on the top of `other` pixel.
    fn alpha_mix(self, other: Self) -> Self;

    /// mix two color linearly with a float number, with mix `self` and (1-mix) `other`. If mix is outside range of [0, 1], black will be returned
    fn color_mix(self, other: Self, mix: f32) -> Self;

    fn get_alpha(&self) -> u8;

    fn get_red(&self) -> u8;

    fn get_green(&self) -> u8;

    fn get_blue(&self) -> u8;
}

impl PixelMixer for AlphaPixel {
    /// mix two color using alpha channel composition, supposing `self` is on the top of `other` pixel.
    fn alpha_mix(self, other: Self) -> Self {
        let alpha = self.get_alpha() as u16;
        let red = self.get_red();
        let green = self.get_green();
        let blue = self.get_blue();
        let ori_red = other.get_red();
        let ori_green = other.get_green();
        let ori_blue = other.get_blue();
        let new_red = (((red as u16) * (255 - alpha) + (ori_red as u16) * alpha) / 255) as u8;
        let new_green = (((green as u16) * (255 - alpha) + (ori_green as u16) * alpha) / 255) as u8;
        let new_blue = (((blue as u16) * (255 - alpha) + (ori_blue as u16) * alpha) / 255) as u8;
        return new_alpha_pixel(other.get_alpha(), new_red, new_green, new_blue);
    }

    /// mix two color linearly with a float number, with mix `self` and (1-mix) `other`. If mix is outside range of [0, 1], black will be returned
    fn color_mix(self, other: Self, mix: f32) -> Self {
        if mix < 0f32 || mix > 1f32 {  // cannot mix value outside [0, 1]
            return BLACK;
        }
        let new_alpha = ((self.get_alpha() as f32) * mix + (other.get_alpha() as f32) * (1f32-mix)) as u8;
        let new_red = ((self.get_red() as f32) * mix + (other.get_red() as f32) * (1f32-mix)) as u8;
        let new_green = ((self.get_green() as f32) * mix + (other.get_green() as f32) * (1f32-mix)) as u8;
        let new_blue = ((self.get_blue() as f32) * mix + (other.get_blue() as f32) * (1f32-mix)) as u8;
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

pub fn new_alpha_pixel(alpha: u8, red: u8, green: u8, blue: u8) -> AlphaPixel {
    return ((alpha as u32) << 24) +
        ((red as u32) << 16) +
        ((green as u32) << 8) +
        (blue as u32) 
}