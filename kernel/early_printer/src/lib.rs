//! A basic ASCII text printer for displaying text on a graphical framebuffer during early boot.
//!
//! Does not support user scrolling, cursors, or any other advanced terminal features.

#![no_std]

use core::{fmt::{self, Write}, slice, ops::{Deref, DerefMut}};
use boot_info::{FramebufferInfo, Address};
use font::FONT_BASIC;
use memory::{BorrowedSliceMappedPages, Mutable, PteFlags};
use spin::Mutex;

/// The height in pixels that each character occupies, not including any padding.
const CHARACTER_HEIGHT: u32 = font::CHARACTER_HEIGHT as u32;
/// The width in pixels that each character occupies, including 1 pixel of padding.
const CHARACTER_WIDTH: u32 = font::CHARACTER_WIDTH as u32;
/// The width in pixels that each character occupies, excluding padding.
const GLPYH_WIDTH: u32 = CHARACTER_WIDTH - 1;

/// The system-wide framebuffer for early text printing.
static EARLY_FRAMEBUFFER_PRINTER: Mutex<Option<EarlyFramebufferPrinter>> = Mutex::new(None);

/// Initializes a simple graphical framebuffer for early text printing.
///
/// # Usage
/// There are two cases in which this function can be called:
/// 1. If the given framebuffer info provides the framebuffer's virtual address,
///    then the bootloader has already mapped the framebuffer for us.
///    * In this case, this function can be called *before* the memory subsystem
///      has been initialized.
/// 2. If the given framebuffer info provides a framebuffer's physical address,
///    then the bootloader has not mapped anything for us, and thus this function
///    will attempt to allocate and map a new virtual address to that framebuffer.
///    * In this case, this function can only be called *after* the memory subsystem
///      has been initialized.
pub fn init(info: &FramebufferInfo) -> Result<(), &'static str> {
    log::info!("EarlyFramebuffer::init(): {:?}", info);

    if EARLY_FRAMEBUFFER_PRINTER.lock().is_some() {
        return Err("The early framebuffer printer has already been initialized");
    }

    let fb_pixel_count = (info.stride * info.height) as usize;
    let fb_byte_count  = info.bits_per_pixel as usize * fb_pixel_count;

    let fb_memory = match info.address {
        Address::Virtual(vaddr) => {
            // SAFETY: we have no alternative but to trust the bootloader-provided address.
            let slc = unsafe {
                slice::from_raw_parts_mut(vaddr.value() as *mut _, fb_pixel_count)
            };
            FramebufferMemory::Slice(slc)
        }
        Address::Physical(paddr) => {
            let kernel_mmi = memory::get_kernel_mmi_ref().ok_or(
                "BUG: early framebuffer printer cannot map framebuffer's \
                physical address before the memory subsystem is initialized."
            )?;
            let frames = memory::allocate_frames_by_bytes_at(paddr, fb_byte_count)
                .map_err(|_| "couldn't allocate frames for early framebuffer printer")?;
            let pages = memory::allocate_pages(frames.size_in_frames())
                .ok_or("couldn't allocate pages for early framebuffer printer")?;
            let mp = kernel_mmi.lock().page_table.map_allocated_pages_to(
                pages,
                frames,
                PteFlags::new().valid(true).writable(true).device_memory(true)
            )?;
            FramebufferMemory::Mapping(
                mp.into_borrowed_slice_mut(0, fb_pixel_count).map_err(|(_mp, s)| s)?
            )
        }
    };

    let early_fb = EarlyFramebufferPrinter {
        fb: fb_memory,
        width: info.width,
        height: info.height,
        stride: info.stride,
        curr_pixel: PixelCoord { x: 0, y: 0 },
    };
    *EARLY_FRAMEBUFFER_PRINTER.lock() = Some(early_fb);
    Ok(())
}

/// An abstraction over the underlying framebuffer memory that derefs into a slice of pixels.
enum FramebufferMemory {
    Slice(&'static mut [u32]),
    Mapping(BorrowedSliceMappedPages<u32, Mutable>),
}
impl Deref for FramebufferMemory {
    type Target = [u32];
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Slice(slc) => slc,
            Self::Mapping(bmp) => bmp.deref(),
        }
    }
}
impl DerefMut for FramebufferMemory {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Slice(slc) => slc,
            Self::Mapping(bmp) => bmp.deref_mut(),
        }
    }
}

/// A 2-D coordinate into the framebuffer's pixel array,
/// in which `(0,0)` is the top left corner.
#[derive(Copy, Clone)]
struct PixelCoord {
    x: u32,
    y: u32,
}

/// A text printer for writing characters to an early graphical framebuffer.
pub struct EarlyFramebufferPrinter {
    /// The underlying framebuffer memory, accessible as a slice of pixels.
    fb: FramebufferMemory,
    /// The width in pixels of the framebuffer.
    width: u32,
    /// The height in pixels of the framebuffer.
    height: u32,
    /// The stride in pixels of the framebuffer.
    stride: u32,
    /// The current pixel coordinate where the next character will be printed.
    curr_pixel: PixelCoord,
}

impl EarlyFramebufferPrinter {
    /// Prints the given character to the current location in this framebuffer.
    pub fn print_char(
        &mut self,
        ch: char,
        foreground_pixel_color: u32,
        background_pixel_color: u32,
    ) {
        if ch == '\n' {
            return self.newline(background_pixel_color);
        }

        let ascii = if ch.is_ascii() { ch as u8 } else { b'?' };
        let glyph = &FONT_BASIC[ascii as usize];

        for (row_bits, row) in glyph.into_iter().zip(0u32..) {
            // Copy each row of the font glyph to the framebuffer in a single action
            let mut pixel_row: [u32; CHARACTER_WIDTH as usize] = [background_pixel_color; CHARACTER_WIDTH as usize];
            for (pixel, col) in pixel_row.iter_mut().zip(0 .. GLPYH_WIDTH) {
                if (row_bits & (0x80 >> col)) != 0 {
                    *pixel = foreground_pixel_color;
                };
            }
            let start_idx = (self.curr_pixel.y + row) * self.stride + self.curr_pixel.x;
            let fb_row_range = start_idx as usize .. (start_idx + CHARACTER_WIDTH) as usize;
            self.fb[fb_row_range].copy_from_slice(&pixel_row);
        }

        self.advance_by_one_char(background_pixel_color);
    }

    /// Advances the current pixel location by one character, wrapping to the next line if needed.
    fn advance_by_one_char(&mut self, background_pixel_color: u32) {
        let next_col = self.curr_pixel.x + CHARACTER_WIDTH;
        if next_col >= self.width {
            return self.newline(background_pixel_color);
        }

        self.curr_pixel.x = next_col;
    }

    /// Advances the current pixel location to the start of the next line, scrolling if needed.
    fn newline(&mut self, background_pixel_color: u32) {
        // Fill the rest of the current row starting from the current column.
        self.fill_character_line(self.curr_pixel, background_pixel_color);

        self.curr_pixel.x = 0;
        let next_row = self.curr_pixel.y + CHARACTER_HEIGHT;
        if next_row >= self.height {
            return self.scroll(background_pixel_color);
        }

        self.curr_pixel.y = next_row;
    }

    /// Scrolls the text on screen by one line.
    fn scroll(&mut self, background_pixel_color: u32) {
        let start_of_line_two = CHARACTER_HEIGHT * self.stride;
        let end_of_last_line = self.height * self.stride;
        self.fb.copy_within(
            start_of_line_two as usize .. end_of_last_line as usize,
            0,
        );
        let start_of_last_line = self.height - CHARACTER_HEIGHT;
        self.curr_pixel = PixelCoord { x: 0, y: start_of_last_line };
        self.fill_character_line(self.curr_pixel, background_pixel_color);
    }

    /// Fills a full character line;s worth of pixels from the `start_pixel` coordinate
    /// with the given `background_pixel_color`.
    ///
    /// Does not advance or otherwise modify the current pixel location.
    fn fill_character_line(
        &mut self,
        start_pixel: PixelCoord,
        background_pixel_color: u32,
    ) {
        let row_remainder_len = self.width - start_pixel.x;
        for row in 0 .. CHARACTER_HEIGHT {
            let start_idx = (start_pixel.y + row) * self.stride + start_pixel.x;
            let end_idx = start_idx + row_remainder_len;
            self.fb[start_idx as usize .. end_idx as usize].fill(background_pixel_color);
        }
    }
}

impl Write for EarlyFramebufferPrinter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        pub const BLUE: u32 = 0x0000FF;
        pub const LIGHT_GRAY: u32 = 0xD3D3D3;

        for ch in s.chars() {
            self.print_char(ch, BLUE, LIGHT_GRAY);
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        let _ = $crate::print_args_raw(format_args!($($arg)*));
    });
}

#[macro_export]
macro_rules! println {
    ($fmt:expr) => ($crate::print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::print!(concat!($fmt, "\n"), $($arg)*));
}

#[doc(hidden)]
pub fn print_args_raw(args: fmt::Arguments) -> fmt::Result {
    if let Some(early_fb) = EARLY_FRAMEBUFFER_PRINTER.lock().as_mut() {
        early_fb.write_fmt(args)
    } else {
        Ok(())
    }
}
