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

use alloc::boxed::Box;
use core::ops::DerefMut;
use memory::{EntryFlags, FrameRange, MappedPages,PhysicalAddress, FRAME_ALLOCATOR};
use owning_ref::BoxRefMut;

/// A `Pixel` is a `u32` broken down into four bytes. 
/// The lower 24 bits of a Pixel specify its RGB color values, while the upper 8bit is an `alpha` channel,
/// in which an `alpha` value of `0` represents an opaque pixel and `0xFF` represents a completely transparent pixel. 
/// The `alpha` value is used in an alpha-blending compositor that supports transparency.
#[repr(C, packed)]
#[derive(Hash, Debug, Clone, Copy)]
pub struct Pixel {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
    pub alpha: u8
}

/// predefined opaque black
pub const BLACK: Pixel = Pixel{ alpha: 0, red: 0, green: 0, blue: 0};
/// predefined opaque white
pub const WHITE: Pixel = Pixel{ alpha: 0, red: 255, green: 255, blue: 255};

// Every pixel is of `Pixel` type, which is 4 byte as defined in `Pixel`
const PIXEL_BYTES: usize = core::mem::size_of::<Pixel>();

/// Initialize the final frame buffer by allocating a block of memory and map it to the physical framebuffer frames.
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

    /// fill the entire frame buffer with given `color`
    pub fn fill_color(&mut self, color: Pixel) {
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
        self.buffer[idx] = self.buffer[idx].alpha_mix(color);
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

    /// draw a circle on the screen with alpha. (`xc`, `yc`) is the position of the center of the circle, and `r` is the radius
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

impl Pixel {
    /// mix two color using alpha channel composition, supposing `self` is on the top of `other` pixel.
    pub fn alpha_mix(self, other: Self) -> Self {
        let alpha = self.alpha as u16;
        let red = self.red;
        let green = self.green;
        let blue = self.blue;
        let ori_red = other.red;
        let ori_green = other.green;
        let ori_blue = other.blue;
        let new_red = (((red as u16) * (255 - alpha) + (ori_red as u16) * alpha) / 255) as u8;
        let new_green = (((green as u16) * (255 - alpha) + (ori_green as u16) * alpha) / 255) as u8;
        let new_blue = (((blue as u16) * (255 - alpha) + (ori_blue as u16) * alpha) / 255) as u8;
        Self {
            alpha: other.alpha, 
            red: new_red,
            green: new_green,
            blue: new_blue
        }
    }

    /// mix two color linearly with a float number, with mix `self` and (1-mix) `other`. If mix is outside range of [0, 1], black will be returned
    pub fn color_mix(self, other: Self, mix: f32) -> Self {
        if mix < 0f32 || mix > 1f32 {  // cannot mix value outside [0, 1]
            return BLACK;
        }
        let new_alpha = ((self.alpha as f32) * mix + (other.alpha as f32) * (1f32-mix)) as u8;
        let new_red = ((self.red as f32) * mix + (other.red as f32) * (1f32-mix)) as u8;
        let new_green = ((self.green as f32) * mix + (other.green as f32) * (1f32-mix)) as u8;
        let new_blue = ((self.blue as f32) * mix + (other.blue as f32) * (1f32-mix)) as u8;
        Pixel {
            alpha: new_alpha, 
            red: new_red,
            green: new_green,
            blue: new_blue
        }
    }
}

impl From<u32> for Pixel {
    fn from(item: u32) -> Self {
        Pixel {
            alpha: (item >> 24) as u8,
            red: (item >> 16) as u8,
            green: (item >> 8) as u8,
            blue: item as u8
        }
    }
}
