//! A basic ASCII text printer for early text output to a graphical framebuffer
//! or text-mode VGA display.
//!
//! Does not support user scrolling, cursors, or any other advanced features.

#![no_std]
#![feature(let_chains)]

#[cfg(all(feature = "bios", not(target_arch = "x86_64")))]
compile_error!("The `bios` feature can only be used on x86_64");

use core::{fmt::{self, Write}, slice, ops::{Deref, DerefMut}};
use boot_info::{FramebufferInfo, FramebufferFormat};
use font::FONT_BASIC;
use memory::{BorrowedSliceMappedPages, Mutable, PteFlags, PhysicalAddress, PteFlagsArch, PageTable};
use spin::Mutex;

/// The height in pixels that each character occupies, not including any padding.
const CHARACTER_HEIGHT: u32 = font::CHARACTER_HEIGHT as u32;
/// The width in pixels that each character occupies, including 1 pixel of padding.
const CHARACTER_WIDTH: u32 = font::CHARACTER_WIDTH as u32;
/// The width in pixels that each character occupies, excluding padding.
const GLPYH_WIDTH: u32 = CHARACTER_WIDTH - 1;

/// The system-wide printer for early text output to the screen.
static EARLY_FRAMEBUFFER_PRINTER: Mutex<Option<EarlyPrinter>> = {
    #[cfg(feature = "bios")] {
        Mutex::new(Some(EarlyPrinter::VgaTextMode(vga_buffer::VgaBuffer::new())))
    }
    #[cfg(not(feature = "bios"))] {
        Mutex::new(None)
    }
};

/// The early printer can either use a graphical framebuffer or text-mode VGA.
enum EarlyPrinter {
    Framebuffer(EarlyFramebufferPrinter),
    #[cfg(feature = "bios")]
    VgaTextMode(vga_buffer::VgaBuffer),
}
/// Forward the `fmt::Write` trait through this enum.
impl Write for EarlyPrinter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        match self {
            Self::Framebuffer(efb) => efb.write_str(s),
            #[cfg(feature = "bios")]
            Self::VgaTextMode(vga) => vga.write_str(s),
        }
    }
}

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
///
/// It is valid (and recommended) to call this function in both circumstances
/// described above, i.e., right after taking over from the bootloader AND again
/// after the memory subsystem has been initialized.
pub fn init(
    info: &FramebufferInfo,
    page_table: Option<&mut PageTable>,
) -> Result<(), &'static str> {
    if matches!(info.format, FramebufferFormat::TextCharacter) {
        log::debug!("Skipping `early_printer::init()` for text-mode VGA");
        return Ok(());
    }
    let fb_pixel_count = (info.stride * info.height) as usize;
    let mut flags_used = None;
    let mut staging_fb_range = None;
    let use_vaddr = page_table.is_none();

    let (fb_paddr, fb_memory, staging_fb) = if use_vaddr && let Some(vaddr) = info.virt_addr {
        let paddr = memory::translate(vaddr)
            .ok_or("BUG: bootloader-provided framebuffer virtual address wasn't mapped!")?;
        if paddr != info.phys_addr {
            log::error!("Mismatch! paddr: {:#X}, phys_addr: {:#X}", paddr, info.phys_addr);
            return Err("BUG: bootloader invalidly mapped the early framebuffer");
        }
        // SAFETY: we checked that the bootloader-provided address was mapped,
        //         but we have no real alternative but to trust that it maps a framebuffer.
        let slc = unsafe {
            slice::from_raw_parts_mut(vaddr.value() as *mut u32, fb_pixel_count)
        };
        (
            info.phys_addr,
            FramebufferMemory::Slice(slc),
            None,
        )
    } else {
        let pg_tbl = page_table.ok_or(
            "BUG: early framebuffer printer cannot map framebuffer's \
            physical address before the memory subsystem is initialized."
        )?;
        let frames = memory::allocate_frames_by_bytes_at(
            info.phys_addr,
            info.total_size_in_bytes as usize,
        ).map_err(|_| "couldn't allocate frames for early framebuffer printer")?;
        let num_pages = frames.size_in_frames();
        let pages = memory::allocate_pages(num_pages)
            .ok_or("couldn't allocate pages for early framebuffer printer")?;
        let mut flags: PteFlagsArch = PteFlags::new()
            .valid(true)
            .writable(true)
            .into();

        #[cfg(target_arch = "x86_64")] {
            if page_attribute_table::init().is_ok() {
                flags = flags.pat_index(
                    page_attribute_table::MemoryCachingType::WriteCombining.pat_slot_index()
                );
            } else {
                flags = flags.device_memory(true);
            }
        }
        #[cfg(not(target_arch = "x86_64"))] {
            flags = flags.device_memory(true);
        }

        flags_used = Some(flags);
        let mp = pg_tbl.map_allocated_pages_to(pages, frames, flags)?;
        let fb_memory = FramebufferMemory::Mapping(
            mp.into_borrowed_slice_mut(0, fb_pixel_count).map_err(|(_mp, s)| s)?
        );

        // Attempt to allocate a staging framebuffer, which is used to significantly
        // accelerate scrolling by not having to read from the framebuffer memory.
        let staging_fb = memory::allocate_pages(num_pages)
            .and_then(|pages|
                pg_tbl.map_allocated_pages(
                    pages,
                    PteFlags::new().valid(true).writable(true),
                )
                .ok()
            )
            .and_then(|mp| {
                staging_fb_range = Some(mp.deref().clone());
                mp.into_borrowed_slice_mut(0, fb_pixel_count).ok()
            });
            
        (info.phys_addr, fb_memory, staging_fb)
    };

    // Use the current pixel coordinate if the early_printer has already been intiialized,
    // such that we continue where the existing printer left off.
    let curr_pixel = {
        if let Some(EarlyPrinter::Framebuffer(ep)) = EARLY_FRAMEBUFFER_PRINTER.lock().deref() {
            ep.curr_pixel
        } else {
            PixelCoord { x: 0, y: 0 }
        }
    };

    // Round down the height of the fb to the nearest multiple of `CHARACTER_HEIGHT`
    // in order to prevent characters from being displayed partially offscreen
    let height = (info.height / CHARACTER_HEIGHT) * CHARACTER_HEIGHT;
    let mut early_fb = EarlyFramebufferPrinter {
        fb: fb_memory,
        staging_fb,
        paddr: fb_paddr,
        width: info.width,
        height,
        stride: info.stride,
        format: info.format,
        curr_pixel,
    };

    let _res = early_fb.write_fmt(format_args!(
        "Initialized early printer with framebuffer:
        paddr: {:#X}
        resolution: {} x {}  (stride {}, capped height {})
        format: {:?}
        flags: {:?}
        staging_fb: {:X?}\n",
        fb_paddr,
        info.width, info.height, info.stride, height,
        info.format,
        flags_used,
        staging_fb_range,
    ));
    *EARLY_FRAMEBUFFER_PRINTER.lock() = Some(EarlyPrinter::Framebuffer(early_fb));
    Ok(())
}

/// De-initializes and returns the early graphical framebuffer,
/// allowing it to be re-used elsewhere.
#[doc(alias("deinit", "clean up"))]
pub fn take() -> Option<EarlyFramebufferPrinter> {
    if let Some(EarlyPrinter::Framebuffer(early_fb)) = EARLY_FRAMEBUFFER_PRINTER.lock().take() {
        Some(early_fb)
    } else {
        None
    }
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
    /// The optional staging buffer, used to accelerate scrolling.
    staging_fb: Option<BorrowedSliceMappedPages<u32, Mutable>>,
    /// The starting physical address of the framebuffer.
    pub paddr: PhysicalAddress,
    /// The width in pixels of the framebuffer.
    pub width: u32,
    /// The height in pixels of the framebuffer.
    pub height: u32,
    /// The stride in pixels of the framebuffer.
    pub stride: u32,
    /// The format of this framebuffer.
    pub format: FramebufferFormat,
    /// The current pixel coordinate where the next character will be printed.
    curr_pixel: PixelCoord,
}

impl EarlyFramebufferPrinter {
    /// Returns the memory mapping for the underlying framebuffer, allowing it to be reused.
    pub fn into_mapping(self) -> Option<BorrowedSliceMappedPages<u32, Mutable>> {
        match self.fb {
            FramebufferMemory::Mapping(m) => Some(m),
            _ => None,
        }
    }

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

            // Write to the staging fb if we have one, otherwise write to the main fb.
            let dest_fb = self.staging_fb.as_deref_mut().unwrap_or(self.fb.deref_mut());
            dest_fb[fb_row_range].copy_from_slice(&pixel_row);
        }

        self.advance_by_one_char(background_pixel_color);
    }

    /// Advances the current pixel location by one character.
    ///
    /// If another character wouldn't fit at the next location, it wraps to the next line.
    fn advance_by_one_char(&mut self, background_pixel_color: u32) {
        let next_col = self.curr_pixel.x + CHARACTER_WIDTH;
        self.curr_pixel.x = next_col;
        if next_col + CHARACTER_WIDTH >= self.width {
            return self.newline(background_pixel_color);
        }
    }

    /// Advances the current pixel location to the start of the next line, scrolling if needed.
    fn newline(&mut self, background_pixel_color: u32) {
        // Fill the rest of the current row starting from the current column.
        self.fill_character_line(self.curr_pixel, background_pixel_color);
        self.curr_pixel.x = 0;

        self.copy_line_from_staging_fb();

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
        let src_range = start_of_line_two as usize .. end_of_last_line as usize;

        if let Some(staging_fb) = self.staging_fb.as_deref_mut() {
            // Scroll up the staging buffer.
            staging_fb.copy_within(src_range, 0);
        }
        else {
            // If we don't have a staging fb, we must scroll within the main fb itself.
            self.fb.copy_within(src_range, 0);
        }
        let start_of_last_line = self.height - CHARACTER_HEIGHT;
        self.curr_pixel = PixelCoord { x: 0, y: start_of_last_line };
        self.fill_character_line(self.curr_pixel, background_pixel_color);

        if let Some(staging_fb) = self.staging_fb.as_deref() {
            // Copy from the staging fb (if we have one) to the main fb.
            self.fb.copy_from_slice(&staging_fb);
        }
    }

    /// Fills a full character line's worth of pixels from the `start_pixel` coordinate
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
            // Write to the staging fb if we have one, otherwise write to the main fb.
            let dest_fb = self.staging_fb.as_deref_mut().unwrap_or(self.fb.deref_mut());
            dest_fb[start_idx as usize .. end_idx as usize].fill(background_pixel_color);
        }
    }

    /// Copies a full character line's worth of pixels from the staging fb to the main fb.
    ///
    /// If there is no staging fb, this does nothing.
    fn copy_line_from_staging_fb(&mut self) {
        if let Some(staging_fb) = self.staging_fb.as_deref() {
            let start_idx = self.curr_pixel.y * self.stride;
            let end_idx = start_idx + (CHARACTER_HEIGHT * self.stride);
            let range = start_idx as usize .. end_idx as usize;
            self.fb[range.clone()].copy_from_slice(&staging_fb[range]);
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

/// Prints the formatted output to the early framebuffer writer,
/// if it has been initialized.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        let _ = $crate::print_args_raw(format_args!($($arg)*));
    });
}

/// Prints the formatted output with an appended newline ('\n')
/// to the early framebuffer writer, if it has been initialized.
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
