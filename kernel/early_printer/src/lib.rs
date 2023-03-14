//! A basic ASCII text printer for displaying text on the screen during early boot.
//!
//! Does not support scrolling, cursors, colors, or any other advanced features.

#![no_std]

use core::{fmt, slice, ops::{Deref, DerefMut}};
use memory::{BorrowedSliceMappedPages, Mutable, PteFlags};
use spin::Mutex;
use boot_info::{FramebufferInfo, Address};

static EARLY_FRAMEBUFFER: Mutex<Option<EarlyFramebuffer>> = Mutex::new(None);

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

    if EARLY_FRAMEBUFFER.lock().is_some() {
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

    let early_fb = EarlyFramebuffer {
        fb: fb_memory,
        width: info.width,
        height: info.height,
        stride: info.stride,
        next_row: 0,
        next_col: 0,
    };

    *EARLY_FRAMEBUFFER.lock() = Some(early_fb);

    Ok(())
}

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

pub struct EarlyFramebuffer {
    fb: FramebufferMemory,
    width: u32,
    height: u32,
    stride: u32,
    next_row: u32,
    next_col: u32,
}

/*
impl<'fb> EarlyFramebufferTextPrinter<'fb> {
    /// Prints a string in a framebuffer.
    /// Returns (column, line, rectangle), i.e. the position of the next symbol and an rectangle which covers the updated area.
    /// A block item (index, width) represents the index of line number and the width of charaters in this line as pixels. It can be viewed as a framebuffer block which is described in the `framebuffer_compositor` crate.
    /// # Arguments
    /// * `framebuffer`: the framebuffer to display in.
    /// * `coordinate`: the left top coordinate of the text block relative to the origin(top-left point) of the framebuffer.
    /// * `width`, `height`: the size of the text block in number of pixels.
    /// * `slice`: the string to display.
    /// * `fg_pixel`: the value of pixels in the foreground.
    /// * `bg_pixel` the value of pixels in the background.
    /// * `column`, `line`: the location of the text in the text block in number of characters.
    pub fn print_string<P: Pixel>(
        framebuffer: &mut Framebuffer<P>,
        coordinate: Coord,
        width: usize,
        height: usize,
        slice: &str,
        fg_pixel: P,
        bg_pixel: P,
        column: usize,
        line: usize,
    ) -> (usize, usize, Rectangle) {
        let buffer_width = width / CHARACTER_WIDTH;
        let buffer_height = height / CHARACTER_HEIGHT;
        let (x, y) = (coordinate.x, coordinate.y);

        let mut curr_line = line;
        let mut curr_column = column;

        let top_left = Coord::new(0, (curr_line * CHARACTER_HEIGHT) as isize);

        for byte in slice.bytes() {
            if byte == b'\n' {
                let mut blank = Rectangle {
                    top_left: Coord::new(
                        coordinate.x + (curr_column * CHARACTER_WIDTH) as isize,
                        coordinate.y + (curr_line * CHARACTER_HEIGHT) as isize,
                    ),
                    bottom_right: Coord::new(
                        coordinate.x + width as isize,
                        coordinate.y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
                    )
                };
                // fill the remaining blank of current line and go to the next line
                fill_blank(
                    framebuffer,
                    &mut blank,
                    bg_pixel,
                );
                curr_column = 0;
                curr_line += 1;
                if curr_line == buffer_height {
                    break;
                }
            } else {
                if curr_column == buffer_width {
                    curr_column = 0;
                    curr_line += 1;
                    if curr_line == buffer_height {
                        break;
                    }
                }
                // print the next character
                print_ascii_character(
                    framebuffer,
                    byte,
                    fg_pixel,
                    bg_pixel,
                    coordinate,
                    curr_column,
                    curr_line,
                );
                curr_column += 1;
            }
        }  

        let mut blank = Rectangle {
            top_left: Coord::new(
                x + (curr_column * CHARACTER_WIDTH) as isize,
                y + (curr_line * CHARACTER_HEIGHT) as isize,
            ),
            bottom_right: Coord::new(
                x + width as isize,
                y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
            )
        };
        // fill the blank of the last line
        fill_blank(
            framebuffer,
            &mut blank,
            bg_pixel,
        );

        let bottom_right = Coord::new(
            (buffer_width * CHARACTER_WIDTH) as isize, 
            ((curr_line + 1) * CHARACTER_HEIGHT) as isize
        );

        let update_area = Rectangle {
            top_left: top_left,
            bottom_right: bottom_right,
        };

        // fill the blank of the remaining part
        blank = Rectangle {
            top_left: Coord::new(
                x,
                y + ((curr_line + 1) * CHARACTER_HEIGHT) as isize,
            ),
            bottom_right: Coord::new(
                x + width as isize,
                y + height as isize,
            )
        };
        fill_blank(
            framebuffer,
            &mut blank,
            bg_pixel,
        );

        // return the position of next symbol and updated blocks.
        (curr_column, curr_line, update_area)
    }

    /// Prints a character to the framebuffer at position (line, column) of all characters in the text area.
    /// # Arguments
    /// * `framebuffer`: the framebuffer to display in.
    /// * `character`: the ASCII code of the character to display.
    /// * `fg_pixel`: the value of every pixel in the character.
    /// * `bg_color`: the value of every pixel in the background.
    /// * `coordinate`: the left top coordinate of the text block relative to the origin(top-left point) of the framebuffer.
    /// * `column`, `line`: the location of the character in the text block as symbols.
    pub fn print_ascii_character<P: Pixel>(
        framebuffer: &mut Framebuffer<P>,
        character: ASCII,
        fg_pixel: P,
        bg_pixel: P,
        coordinate: Coord,
        column: usize,
        line: usize,
    ) {
        let start = coordinate + ((column * CHARACTER_WIDTH) as isize, (line * CHARACTER_HEIGHT) as isize);
        if !framebuffer.overlaps_with(start, CHARACTER_WIDTH, CHARACTER_HEIGHT) {
            return
        }
        // print from the offset within the framebuffer
        let (buffer_width, buffer_height) = framebuffer.get_size();
        let off_set_x: usize = if start.x < 0 { -(start.x) as usize } else { 0 };
        let off_set_y: usize = if start.y < 0 { -(start.y) as usize } else { 0 };    
        let mut j = off_set_x;
        let mut i = off_set_y;
        loop {
            let coordinate = start + (j as isize, i as isize);
            if framebuffer.contains(coordinate) {
                let pixel = if j >= 1 {
                    // leave 1 pixel gap between two characters
                    let index = j - 1;
                    let char_font = font::FONT_BASIC[character as usize][i];
                    if get_bit(char_font, index) != 0 {
                        fg_pixel
                    } else {
                        bg_pixel
                    }
                } else {
                    bg_pixel
                };
                framebuffer.draw_pixel(coordinate, pixel);
            }
            j += 1;
            if j == CHARACTER_WIDTH || start.x + j as isize == buffer_width as isize {
                i += 1;
                if i == CHARACTER_HEIGHT || start.y + i as isize == buffer_height as isize {
                    return
                }
                j = off_set_x;
            }
        }
    }
}
*/


#[macro_export]
macro_rules! print_raw {
    ($($arg:tt)*) => ({
        let _ = $crate::print_args_raw(format_args!($($arg)*));
    });
}

#[macro_export]
macro_rules! println_raw {
    ($fmt:expr) => ($crate::print_raw!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::print_raw!(concat!($fmt, "\n"), $($arg)*));
}

#[doc(hidden)]
pub fn print_args_raw(args: fmt::Arguments) -> fmt::Result {
    todo!("EarlyPrinter::print_args_raw!() is unimplemented");

    // use core::fmt::Write;
    // EARLY_FRAMEBUFFER.lock().write_fmt(args)
}

