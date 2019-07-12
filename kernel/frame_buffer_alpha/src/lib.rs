//! This crate is a frame buffer manager.
//! * It defines a FrameBufferAlpha structure and creates new framebuffers for applications

#![no_std]

#[macro_use] extern crate log;

extern crate multicore_bringup;
extern crate spin;
extern crate alloc;
extern crate memory;
extern crate owning_ref;
extern crate font;

use alloc::boxed::Box;
use core::ops::DerefMut;
use memory::{EntryFlags, FrameRange, MappedPages,PhysicalAddress, FRAME_ALLOCATOR};
use owning_ref::BoxRefMut;

/// A Pixel is a u32 integer. The lower 24 bits of a Pixel specifie the RGB color of a pixel
/// , while the first 8bit is alpha channel which helps to composite windows
/// alpha = 0 means opaque and 0xFF means transparent
pub type Pixel = u32;

// Every pixel is of u32 type
const PIXEL_BYTES: usize = 4;

/// Initialize the final frame buffer.
/// Allocate a block of memory and map it to the physical framebuffer frames.
/// Init the frame buffer. Allocate a block of memory and map it to the frame buffer frames.
pub fn init() -> Result<FrameBufferAlpha, &'static str> {
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

    // Initialize the final framebuffer
    let framebuffer = FrameBufferAlpha::new(buffer_width, buffer_height, Some(vesa_display_phys_start))?;

    Ok(framebuffer)
}

/// The virtual frame buffer struct. It contains the size of the buffer and a buffer array
#[derive(Hash)]
pub struct FrameBufferAlpha {
    pub width: usize,
    pub height: usize,
    pub buffer: BoxRefMut<MappedPages, [Pixel]>,
}

impl FrameBufferAlpha {
    /// Create a new virtual frame buffer with specified size.
    /// If the physical_address is specified, the new virtual frame buffer will be mapped to hardware's physical memory at that address.
    /// If the physical_address is none, the new function will allocate a block of physical memory at a random address and map the new frame buffer to that memory.
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

    /// compute the index of pixel (x, y) in the buffer array
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    /// check if a pixel (x,y) is within the framebuffer
    pub fn check_in_buffer(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height
    }

    /// get one pixel at given position
    pub fn get_pixel(& self, x: usize, y: usize) -> Result<Pixel, &'static str> {
        if ! self.check_in_buffer(x, y) {
            return Err("get pixel out of bound");
        }
        Ok(self.buffer[self.index(x, y)])
    }

    /// fullfill the frame buffer with given color
    pub fn fullfill_color(&mut self, color: Pixel) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.draw_point(x, y, color);
            }
        }
    }

    /// draw a point on this framebuffer
    pub fn draw_point(&mut self, x: usize, y: usize, color: Pixel) {
        let idx = self.index(x, y);
        self.buffer[idx] = color;
    }

    /// draw a point on this framebuffer with alpha
    pub fn draw_point_alpha(&mut self, x: usize, y: usize, color: Pixel) {
        let idx = self.index(x, y);
        self.buffer[idx] = alpha_mix(self.buffer[idx], color);
    }

    /// draw a rectangle on this framebuffer
    pub fn draw_rect(&mut self, xs: usize, xe: usize, ys: usize, ye: usize, color: Pixel) {
        for y in ys..ye {
            for x in xs..xe {
                self.draw_point(x, y, color);
            }
        }
    }

    /// draw a rectangle on this framebuffer with alpha
    pub fn draw_rect_alpha(&mut self, xs: usize, xe: usize, ys: usize, ye: usize, color: Pixel) {
        for y in ys..ye {
            for x in xs..xe {
                self.draw_point_alpha(x, y, color);
            }
        }
    }

    /// draw a circle on the screen with alpha
    pub fn draw_circle_alpha(&mut self, xc: usize, yc: usize, r: usize, color: Pixel) {
        let r2 = (r*r) as isize;
        for y in yc-r..yc+r {
            for x in xc-r..xc+r {
                if self.check_in_buffer(x, y) {
                    let xd = (x-xc) as isize;
                    let yd = (y-yc) as isize;
                    if xd*xd + yd*yd <= r2 {
                        self.draw_point_alpha(x, y, color);
                    }
                }
            }
        }
    }

    /// draw a char on the screen with alpha
    pub fn draw_char_8x16(&mut self, x: usize, y: usize, c: u8, color: Pixel) {
        for yi in 0..16 {
            let char_font: u8 = font::FONT_BASIC[c as usize][yi];
            for xi in 0..8 {
                const HIGHEST_BIT: u8 = 0x80;
                if char_font & (HIGHEST_BIT >> xi) != 0 {
                    self.draw_point_alpha(x+xi, y+yi, color);
                }
            }
        }
    }
}

macro_rules! byte_alpha { ($x:expr) => (($x >> 24) as u8); }
macro_rules! byte_red { ($x:expr) => (($x >> 16) as u8); }
macro_rules! byte_green { ($x:expr) => (($x >> 8) as u8); }
macro_rules! byte_blue { ($x:expr) => ($x as u8); }
/// construct a color with alpha, red, green, blue
fn to_color(alpha: u8, red: u8, green: u8, blue: u8) -> Pixel {
    ((alpha as Pixel) << 24) | ((red as Pixel) << 16) | ((green as Pixel) << 8) | (blue as Pixel)
}

/// mix two color for one pixel on the top of another
pub fn alpha_mix(bottom: Pixel, top: Pixel) -> Pixel {
    let alpha = byte_alpha!(top) as u16;
    let red = byte_red!(top);
    let green = byte_green!(top);
    let blue = byte_blue!(top);
    let ori_red = byte_red!(bottom);
    let ori_green = byte_green!(bottom);
    let ori_blue = byte_blue!(bottom);
    let new_red = (((red as u16) * (255 - alpha) + (ori_red as u16) * alpha) / 255) as u8;
    let new_green = (((green as u16) * (255 - alpha) + (ori_green as u16) * alpha) / 255) as u8;
    let new_blue = (((blue as u16) * (255 - alpha) + (ori_blue as u16) * alpha) / 255) as u8;
    to_color(byte_alpha!(bottom), new_red, new_green, new_blue)
}

/// mix two color linearly with a float number
pub fn color_mix(c1: Pixel, c2: Pixel, mix: f32) -> Pixel {
    if mix < 0f32 || mix > 1f32 {  // cannot mix value outside [0, 1]
        const BLACK: Pixel = 0x00000000;
        return BLACK;
    }
    let alpha1 = byte_alpha!(c1);
    let red1 = byte_red!(c1);
    let green1 = byte_green!(c1);
    let blue1 = byte_blue!(c1);
    let alpha2 = byte_alpha!(c2);
    let red2 = byte_red!(c2);
    let green2 = byte_green!(c2);
    let blue2 = byte_blue!(c2);
    let new_alpha = ((alpha1 as f32) * mix + (alpha2 as f32) * (1f32-mix)) as u8;
    let new_red = ((red1 as f32) * mix + (red2 as f32) * (1f32-mix)) as u8;
    let new_green = ((green1 as f32) * mix + (green2 as f32) * (1f32-mix)) as u8;
    let new_blue = ((blue1 as f32) * mix + (blue2 as f32) * (1f32-mix)) as u8;
    to_color(new_alpha, new_red, new_green, new_blue)
}
