#![no_std]
extern crate memory;

use crate::*;
use core::marker::PhantomData;
use memory::{BorrowedSliceMappedPages, Mutable, PhysicalAddress, PteFlags, PteFlagsArch};

use core::slice::IterMut;
use log::{debug, info};

/// Virtual framebuffer that is not mapped to actual screen pixels, but used as a backbuffer.
pub struct VirtualFrameBuffer {
    pub width: usize,
    pub height: usize,
    pub buffer: BorrowedSliceMappedPages<u32, Mutable>,
}

impl VirtualFrameBuffer {
    pub fn new(width: usize, height: usize) -> Result<VirtualFrameBuffer, &'static str> {
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let size = width * height * core::mem::size_of::<u32>();
        let pages = memory::allocate_pages_by_bytes(size)
            .ok_or("could not allocate pages for a new framebuffer")?;
        let mapped_buffer = kernel_mmi_ref
            .lock()
            .page_table
            .map_allocated_pages(pages, PteFlags::new().valid(true).writable(true))?;
        Ok(VirtualFrameBuffer {
            width,
            height,
            buffer: mapped_buffer
                .into_borrowed_slice_mut(0, width * height)
                .map_err(|(_mp, s)| s)?,
        })
    }

    pub fn blank(&mut self) {
        for pixel in self.buffer.iter_mut() {
            *pixel = 0x000000;
        }
    }

    /// Returns a single `IterMut<T>` from specified `row` of the framebuffer if no row found from given parameters
    /// returns an empty `IterMut<T>`.
    /// 
    /// * `row`  - Specifies which row will be returned from this function
    /// * `rect` - Specifies dimensions of row which will be returned as `IterMut<u32>
    pub fn get_exact_row(&mut self, rect: Rect, row: usize) -> IterMut<u32> {
        let stride = self.width;
        if row >= rect.y as usize && row < rect.y_plus_height() as usize {
            let start_of_row = (stride * row) + rect.x as usize;
            let end_of_row = start_of_row + rect.width as usize;
            let framebuffer_slice = &mut self.buffer;
            if let Some(row_slice) = framebuffer_slice.get_mut(start_of_row..end_of_row) {
                return row_slice.iter_mut();
            }
        }
        [].iter_mut()
    }
}

/// Physical framebuffer we use for final rendering to the screen.
pub struct PhysicalFrameBuffer {
    width: usize,
    height: usize,
    stride: usize,
    pub buffer: BorrowedSliceMappedPages<u32, Mutable>,
}
impl PhysicalFrameBuffer {
    pub(crate) fn init_front_buffer() -> Result<PhysicalFrameBuffer, &'static str> {
        let graphic_info =
            multicore_bringup::get_graphic_info().ok_or("Failed to get graphic info")?;
        if graphic_info.physical_address() == 0 {
            return Err("wrong physical address for porthole");
        }
        let vesa_display_phys_start =
            PhysicalAddress::new(graphic_info.physical_address() as usize)
                .ok_or("Invalid address")?;
        let buffer_width = graphic_info.width() as usize;
        let buffer_height = graphic_info.height() as usize;
        // We are assuming a pixel is 4 bytes big
        let stride = graphic_info.bytes_per_scanline() / 4;

        let framebuffer = PhysicalFrameBuffer::new(
            buffer_width,
            buffer_height,
            stride as usize,
            vesa_display_phys_start,
        )?;
        Ok(framebuffer)
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    fn new(
        width: usize,
        height: usize,
        stride: usize,
        physical_address: PhysicalAddress,
    ) -> Result<PhysicalFrameBuffer, &'static str> {
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let size = width * height * core::mem::size_of::<u32>();
        let pages = memory::allocate_pages_by_bytes(size)
            .ok_or("could not allocate pages for a new framebuffer")?;

        let mapped_framebuffer = {
            let mut flags: PteFlagsArch = PteFlags::new().valid(true).writable(true).into();

            #[cfg(target_arch = "x86_64")]
            {
                let use_pat = page_attribute_table::init().is_ok();
                if use_pat {
                    flags = flags.pat_index(
                        page_attribute_table::MemoryCachingType::WriteCombining.pat_slot_index(),
                    );
                    info!("Using PAT write-combining mapping for real physical framebuffer memory");
                } else {
                    flags = flags.device_memory(true);
                    info!("Falling back to cache-disable mapping for real physical framebuffer memory");
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                flags = flags.device_memory(true);
            }

            let frames = memory::allocate_frames_by_bytes_at(physical_address, size)
                .map_err(|_e| "Couldn't allocate frames for the final framebuffer")?;
            let fb_mp = kernel_mmi_ref
                .lock()
                .page_table
                .map_allocated_pages_to(pages, frames, flags)?;
            debug!("Mapped real physical framebuffer: {fb_mp:?}");
            fb_mp
        };
        Ok(PhysicalFrameBuffer {
            width,
            height,
            stride,
            buffer: mapped_framebuffer
                .into_borrowed_slice_mut(0, width * height)
                .map_err(|(_mp, s)| s)?,
        })
    }
}

/// From given mutable `VirtualFrameBuffer` and `Rect` allows you to mutably iterate
/// rows of mutable slices.
///
/// To help you understand this structure consider this example:
/// Think `VirtualFrameBuffer` as a big cake and `Rect` as a smaller cake within the `VirtualFrameBuffer`
/// this returns row of mutable slices from that smaller cake.
pub struct FramebufferRowChunks<'a> {
    /// Framebuffer we used to get the `rows` from
    framebuffer: &'a mut [u32],
    /// A `Rect` that specifies the dimensions of the row to be extracted from the framebuffer;
    rect: Rect,
    /// Number of pixels in a line of `Framebuffer`
    stride: usize,
    /// the index in the framebuffer at which the current row starts
    start_of_row: usize,
    /// Where we end the row
    end_of_row: usize,
    /// The index of the current row being extracted from the framebuffer
    current_column: usize,
}

impl<'a> FramebufferRowChunks<'a> {
    /// Creates a new `FramebufferRowChunks` from given `rect` and `stride`;
    /// if given `rect.width` is bigger than the given `stride` it will return a row big as the stride.
    pub fn new(framebuffer: &'a mut VirtualFrameBuffer, rect: &mut Rect, stride: usize) -> Self {
        rect.width = core::cmp::min(rect.width, stride);
        let current_column = rect.y as usize;
        let start_of_row = (stride * current_column) + rect.x as usize;
        let end_of_row = (stride * current_column) + rect.x_plus_width() as usize;
        Self {
            framebuffer: &mut framebuffer.buffer,
            rect: *rect,
            stride,
            start_of_row,
            end_of_row,
            current_column,
        }
    }

}

impl<'a> Iterator for FramebufferRowChunks<'a> {
    type Item = &'a mut [u32];

    fn next(&mut self) -> Option<&'a mut [u32]> {
        if self.current_column < self.rect.y_plus_height() as usize {
            // To not fight borrow checker we do this little trick here
            let slice = core::mem::replace(&mut self.framebuffer, &mut []);

            if slice.len() < self.end_of_row {
                return None;
            }
            self.current_column += 1;

            let (row, rest_of_slice) = slice.split_at_mut(self.end_of_row);

            // We want to keep rest of the slice
            self.framebuffer = rest_of_slice;
            if let Some(chunk) = row.get_mut(self.start_of_row..self.end_of_row) {
                // Because we are taking part of a slice we need this gap to be added to
                // `start_of_row` and `end_of_row` so we can correctly index the framebuffer slice
                let gap = self.stride - self.end_of_row;
                self.start_of_row = self.start_of_row + gap;
                self.end_of_row = self.end_of_row + gap;
                return Some(chunk);
            } else {
                None
            }
        } else {
            None
        }
    }
}
