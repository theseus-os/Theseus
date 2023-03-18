use crate::*;
use memory::{BorrowedSliceMappedPages, Mutable, PhysicalAddress, PteFlags, PteFlagsArch};

use core::{
    cmp::min,
    ops::Range,
    slice::{ChunksExactMut, IterMut},
};
use log::{debug, info};

/// An abstraction of a framebuffer.
pub trait Framebuffer {
    /// Returns the width in pixels (not bytes) of this framebuffer.
    fn width(&self) -> usize;

    /// Returns the height in pixels (not bytes) of this framebuffer.
    fn height(&self) -> usize;

    /// Returns the stride in pixels (not bytes) of this framebuffer.
    fn stride(&self) -> usize;

    /// Returns an immutable reference to this framebuffer's underlying pixel array.
    fn buffer(&self) -> &[u32];

    /// Returns a mutable reference to this framebuffer's underlying pixel array.
    fn buffer_mut(&mut self) -> &mut [u32];
}

pub struct StagingFramenbuffer {
    width: usize,
    height: usize,
    stride: usize,
    buffer: BorrowedSliceMappedPages<u32, Mutable>,
}

impl StagingFramenbuffer {
    pub(crate) fn new(
        width: usize,
        height: usize,
        p_framebuffer_stride: usize,
    ) -> Result<StagingFramenbuffer, &'static str> {
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let size = width * height * core::mem::size_of::<u32>();
        let pages = memory::allocate_pages_by_bytes(size)
            .ok_or("could not allocate pages for a new framebuffer")?;
        let mp = kernel_mmi_ref
            .lock()
            .page_table
            .map_allocated_pages(pages, PteFlags::new().valid(true).writable(true))?;
        let buffer = mp
            .into_borrowed_slice_mut(0, (p_framebuffer_stride * height) as usize)
            .map_err(|(_mp, s)| s)?;
        Ok(StagingFramenbuffer {
            width,
            height,
            stride: p_framebuffer_stride,
            buffer,
        })
    }
    pub fn blank(&mut self) {
        for pixel in self.buffer.iter_mut() {
            *pixel = 0x000000;
        }
    }
}

impl Framebuffer for StagingFramenbuffer {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn stride(&self) -> usize {
        self.stride
    }

    fn buffer(&self) -> &[u32] {
        &self.buffer
    }

    fn buffer_mut(&mut self) -> &mut [u32] {
        &mut self.buffer
    }
}

/// A virtual framebuffer is an anonymous chunk of memory not mapped
/// to actual screen pixels.
pub struct VirtualFramebuffer {
    /// The width in pixels of this framebuffer.
    /// This is the same as its stride; virtual framebuffers have no padding bytes.
    width: usize,
    /// The height in pixels of this framebuffer.
    height: usize,
    buffer: BorrowedSliceMappedPages<u32, Mutable>,
}

impl VirtualFramebuffer {
    pub fn new(width: usize, height: usize) -> Result<VirtualFramebuffer, &'static str> {
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let size = width * height * core::mem::size_of::<u32>();
        let pages = memory::allocate_pages_by_bytes(size)
            .ok_or("could not allocate pages for a new framebuffer")?;
        let mp = kernel_mmi_ref
            .lock()
            .page_table
            .map_allocated_pages(pages, PteFlags::new().valid(true).writable(true))?;
        let buffer = mp
            .into_borrowed_slice_mut(0, width * height)
            .map_err(|(_mp, s)| s)?;
        Ok(VirtualFramebuffer {
            width,
            height,
            buffer,
        })
    }

    pub fn blank(&mut self) {
        for pixel in self.buffer.iter_mut() {
            *pixel = 0x000000;
        }
    }

    // Notes to @ouz:
    // * Why do you need both `rect` and `row`?
    //   Doesn't `rect` provide the same info as `row`?
    // * This is entirely redundant with `FramebufferRowIter`.
    //   You can achieve this function by calling `FramebufferRowIter::new(fb, rect).next()`
    //   and then iterating over that row slice.
    //
    //
    /// Returns an iterator over each pixel of the given `row` of the framebuffer.
    ///
    /// If no row found from given parameters
    /// returns an empty `IterMut<T>`.
    ///
    /// * `row`  - Specifies which row will be returned from this function
    /// * `rect` - Specifies dimensions of row which will be returned as `IterMut<u32>
    pub fn get_exact_row(&mut self, rect: Rect, row: usize) -> IterMut<u32> {
        let stride = self.width;
        if row >= rect.y as usize && row < rect.y_plus_height() as usize {
            let start_of_row = (stride * rect.y as usize) + rect.x as usize;
            log::info!("start of row {}", start_of_row);
            let end_of_row = start_of_row + rect.width as usize;
            log::info!("end of row {}", start_of_row + rect.width as usize);
            let framebuffer_slice = &mut self.buffer;
            if let Some(row_slice) = framebuffer_slice.get_mut(start_of_row..end_of_row) {
                //return row_slice.iter_mut();
            }
        }

        let it = FramebufferRowIter::new(self, rect)
            .next()
            .unwrap()
            .iter_mut();
        it
    }
}

impl Framebuffer for VirtualFramebuffer {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn stride(&self) -> usize {
        self.width
    }

    fn buffer(&self) -> &[u32] {
        &self.buffer
    }

    fn buffer_mut(&mut self) -> &mut [u32] {
        &mut self.buffer
    }
}

/// A physical framebuffer is backed by actual graphics memory displayable on screen.
pub struct PhysicalFramebuffer {
    /// The width in pixels of this framebuffer.
    width: usize,
    /// The height in pixels of this framebuffer.
    height: usize,
    /// The stride in pixels of this framebuffer.
    stride: usize,
    buffer: BorrowedSliceMappedPages<u32, Mutable>,
}
impl PhysicalFramebuffer {
    pub(crate) fn init_front_buffer() -> Result<PhysicalFramebuffer, &'static str> {
        let graphic_info =
            multicore_bringup::get_graphic_info().ok_or("Failed to get graphic info")?;
        let physical_address = PhysicalAddress::new(graphic_info.physical_address() as usize)
            .ok_or("Physical framebuffer's physical address was invalid")?;
        let buffer_width = graphic_info.width() as usize;
        let buffer_height = graphic_info.height() as usize;
        let stride_in_pixels = graphic_info.bytes_per_scanline() / graphic_info.bytes_per_pixel();
        PhysicalFramebuffer::new(
            buffer_width,
            buffer_height,
            stride_in_pixels as usize,
            physical_address,
        )
    }

    fn new(
        width: usize,
        height: usize,
        stride: usize,
        physical_address: PhysicalAddress,
    ) -> Result<PhysicalFramebuffer, &'static str> {
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
                .map_err(|_e| "Couldn't allocate frames for the physical framebuffer")?;
            let fb_mp = kernel_mmi_ref
                .lock()
                .page_table
                .map_allocated_pages_to(pages, frames, flags)?;
            debug!("Mapped real physical framebuffer: {fb_mp:?}");
            fb_mp
        };
        Ok(PhysicalFramebuffer {
            width,
            height,
            stride,
            buffer: mapped_framebuffer
                .into_borrowed_slice_mut(0, stride * height)
                .map_err(|(_mp, s)| s)?,
        })
    }
}

impl Framebuffer for PhysicalFramebuffer {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn stride(&self) -> usize {
        self.stride
    }

    fn buffer(&self) -> &[u32] {
        &self.buffer
    }

    fn buffer_mut(&mut self) -> &mut [u32] {
        &mut self.buffer
    }
}

/// An iterator over mutable rows of a framebuffer.
///
/// Each iteration returns a subset of a single framebuffer row based on the
/// rectangle passed into the constructor, up to the width of the whole framebuffer.
pub struct FramebufferRowIter<'a> {
    /// An iterator over the framebuffer's rows.
    /// Each chunk is a slice containing the entire row's pixels.
    rows: ChunksExactMut<'a, u32>,
    /// The range of columns returned from each row, i.e., a subset of that row's pixels.
    column_range: Range<usize>,
}

impl<'a> FramebufferRowIter<'a> {
    /// Creates a new `FramebufferRowIter`, a mutable iterator over a `rectangle` of pixels
    /// in the given `framebuffer`.
    ///
    /// The coordinates given by `rectangle` are relative to that of the given `framebuffer`,
    /// in which `(0,0)` refers to the top-left corner of the `framebuffer`.
    ///
    /// If the `rectangle`'s dimensions exceed the framebuffer's bounds (width and height),
    /// the iteration will be capped at the bounds of the framebuffer.
    pub fn new<F: Framebuffer>(framebuffer: &'a mut F, rect: Rect) -> Self {
        let fb_width = framebuffer.width();
        let fb_height = framebuffer.height();
        let fb_stride = framebuffer.stride();

        let start_row: usize = rect
            .y
            .try_into()
            .expect("Rectangle's y coord was negative. FIXME");
        let end_row = min(start_row + rect.height(), fb_height);

        let start_column: usize = rect
            .x
            .try_into()
            .unwrap_or(0);
        let end_column = min(start_column + rect.width(), fb_width);

        let idx_of_start_row = start_row * fb_stride;
        let idx_of_end_row = end_row * fb_stride;
        let rows =
            framebuffer.buffer_mut()[idx_of_start_row..idx_of_end_row].chunks_exact_mut(fb_width);

        Self {
            rows,
            column_range: start_column..end_column,
        }
    }
}

impl<'a> Iterator for FramebufferRowIter<'a> {
    type Item = &'a mut [u32];

    fn next(&mut self) -> Option<Self::Item> {
        // Notes to @ouz:
        // * This implementation is maybe a *tiny* bit inefficient, at least theoretically.
        //   Right now each chunk contains the entire row rather than the specific range of columns.
        //   That makes the math really easy, but requires us to use `get_mut()` below to
        //   obtain a subset of the columns in that row.
        // * Instead, we could start each stride-sized chunk at the `start_column` in the constructor,
        //   which would allows us to specify only an end column bound,
        //   but then we'd have to handle the final chunk specially, using `into_remainder()`.
        // * This probably is not worth the effort, in fact it might even be slower.
        //   Here, I think the extreme simplicity is well worth it.
        //
        //
        self.rows
            .next()
            .and_then(|full_row| full_row.get_mut(self.column_range.clone()))
    }
}

/// Notes to @ouz: old code of mine showing a simple index-based version of the iterator.
mod index_based_framebuffer_row_iter {
    use super::{min, Framebuffer, Rect};

    /// An iterator over mutable rows of a framebuffer.
    ///
    /// Each iteration returns a subset of a single framebuffer row based on the
    /// rectangle passed into the constructor, up to the width of the whole framebuffer.
    pub struct FramebufferRowIter<'a> {
        /// The underlying memory of the framebuffer whose rows we're iterating over.
        framebuffer: &'a mut [u32],
        /// The stride in pixels of the framebuffer.
        stride: usize,
        /// The width in pixels of the subslice of each row that we return in `next()`.
        subslice_width: usize,
        /// The 1-D index into the framebuffer's pixel array that represents
        /// the start of the subslice returned in the next invocation of `next()`.
        current_column: usize,
    }

    impl<'a> FramebufferRowIter<'a> {
        // Notes to @ouz:
        // * It doesn't make sense to pass in a mutable reference to a Rectangle here.
        //   Why would we want to let this function modify it, and why would the caller care
        //   about those modifications?
        // * Stride is a property of the framebuffer, it doesn't make sense nor should it be possible
        //   to pass in any arbitrary number as the stride value. That would lead to logic/math errors.
        // * Your calculations aren't always correct for the various index values in this constructor;
        //   you conflate the concept of stride, framebuffer width, and desired rectangle width.
        // * The docs for this constructor were extremely vague and formatted incorrectly; see my clarifications.
        // * The `Rect` type is unsuitable for this purpose, since it allows negative indices for `x` and `y`.
        //   Since the `Rect` values are used as an offset from the (0,0) point of the framebuffer's pixel array,
        //   it is invalid to index to an offset *before* the start of the pixel array.
        //   * Also, you converted the types incorrectly. Converting a negative isize into a usize will succeed,
        //     but will be wildly wrong since it will just reinterpret the bit-level representation.
        //     Try it out, you'll see that you need to use try_into() instead.
        //     But really we should use a proper usize type here.
        // * We should probably return a Result instead, such that we can return an error if the given `rect`
        //   is out of the framebuffer's bounds. Otherwise, the caller's expectations may be violated.
        //   * Related to bounds checking, you forgot to check the start bounds of the rectangle too.
        //
        //
        /// Creates a new `FramebufferRowIter`, a mutable iterator over a `rectangle` of pixels
        /// in the given `framebuffer`.
        ///
        /// The coordinates given by `rectangle` are relative to that of the given `framebuffer`,
        /// in which `(0,0)` refers to the top-left corner of the `framebuffer`.
        ///
        /// If the `rectangle`'s dimensions exceed the framebuffer's bounds (width and height),
        /// the iteration will be capped at the bounds of the framebuffer.
        #[allow(dead_code)]
        pub fn new<F: Framebuffer>(framebuffer: &'a mut F, rect: Rect) -> Self {
            let fb_width = framebuffer.width();
            let fb_height = framebuffer.height();
            let fb_stride = framebuffer.stride();

            let start_row: usize = rect
                .y
                .try_into()
                .expect("Rectangle's y coord was negative. FIXME");
            assert!(start_row < fb_height); // FIXME: return result
            let end_row = min(start_row + rect.height(), fb_height);

            let start_column: usize = rect
                .x
                .try_into()
                .expect("Rectangle's x coord was negative. FIXME");
            assert!(start_column < fb_width); // FIXME: return result
            let end_column = min(start_column + rect.width(), fb_width);

            let idx_of_start_row = start_row * fb_stride;
            let idx_of_end_row = end_row * fb_stride;
            let rows_subset = &mut framebuffer.buffer_mut()[idx_of_start_row..idx_of_end_row];

            Self {
                framebuffer: rows_subset,
                stride: fb_stride,
                subslice_width: end_column - start_column,
                current_column: start_column,
            }
        }
    }

    impl<'a> Iterator for FramebufferRowIter<'a> {
        type Item = &'a mut [u32];

        // Notes to @ouz:
        // * This is an index-based implementation, in that I store the current index into the whole framebuffer slice.
        //   I did it this way to illustrate the simplicity herein and how your previous impl was massively overcomplicating it.
        // * It may be more efficient to store the current row slice in the iterator itself so that we can index into the row
        //   rather than starting indexing from the beginning of the entire framebuffer pixel array. I doubt it's significant though.
        fn next(&mut self) -> Option<Self::Item> {
            let start_idx = self.current_column;
            self.current_column += self.stride;

            self.framebuffer
                .get_mut(start_idx..start_idx + self.subslice_width)
                // Notes to @ouz: hack to get around lifetime issues. Obviously we cannot do this in production.
                .map(|sl| unsafe { core::mem::transmute(sl) })
        }
    }
}
