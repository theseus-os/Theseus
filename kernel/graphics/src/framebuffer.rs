use core::{
    borrow::{Borrow, BorrowMut},
    ops::{Deref, DerefMut},
};

use geometry::{Containable, Coordinates, Rectangle};
use memory::{BorrowedSliceMappedPages, Mutable, PhysicalAddress, PteFlags, PteFlagsArch};

use crate::Pixel;

pub struct Framebuffer<P>
where
    P: Pixel,
{
    pub(crate) inner: BorrowedSliceMappedPages<P, Mutable>,
    dimensions: FramebufferDimensions,
}

#[derive(Copy, Clone)]
pub struct FramebufferDimensions {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

impl<P> Framebuffer<P>
where
    P: Pixel,
{
    #[inline]
    pub fn new(
        inner: BorrowedSliceMappedPages<P, Mutable>,
        dimensions: FramebufferDimensions,
    ) -> Self {
        assert!(
            dimensions.width <= dimensions.stride,
            "invalid framebuffer dimensions"
        );
        assert_eq!(
            dimensions.stride * dimensions.height,
            inner.len(),
            "framebuffer dimensions did not match buffer dimensions"
        );

        Self { inner, dimensions }
    }

    #[inline]
    pub fn new_software(dimensions: FramebufferDimensions) -> Self {
        assert!(
            dimensions.width <= dimensions.stride,
            "invalid framebuffer dimensions"
        );

        // TODO Error handling.

        let kernel_mmi_ref = memory::get_kernel_mmi_ref().unwrap();

        let size = dimensions.height * dimensions.stride * core::mem::size_of::<P>();
        let pages = memory::allocate_pages_by_bytes(size).unwrap();
        let mapped_pages = kernel_mmi_ref
            .lock()
            .page_table
            .map_allocated_pages(pages, PteFlags::new().valid(true).writable(true))
            .unwrap();

        Self {
            inner: mapped_pages
                .into_borrowed_slice_mut(0, dimensions.height * dimensions.stride)
                .unwrap(),
            dimensions,
        }
    }

    #[inline]
    pub fn new_hardware(address: PhysicalAddress, dimensions: FramebufferDimensions) -> Self {
        assert!(
            dimensions.width <= dimensions.stride,
            "invalid framebuffer dimensions"
        );

        // TODO Error handling.

        let kernel_mmi_ref = memory::get_kernel_mmi_ref().unwrap();
        let size = dimensions.height * dimensions.stride * core::mem::size_of::<P>();
        let pages = memory::allocate_pages_by_bytes(size).unwrap();

        // For best performance, we map the real physical framebuffer memory
        // as write-combining using the PAT (on x86 only).
        // If PAT isn't available, fall back to disabling caching altogether.
        let mut flags: PteFlagsArch = PteFlags::new().valid(true).writable(true).into();

        #[cfg(target_arch = "x86_64")]
        {
            if page_attribute_table::is_supported() {
                flags = flags.pat_index(
                    page_attribute_table::MemoryCachingType::WriteCombining.pat_slot_index(),
                );
            } else {
                flags = flags.device_memory(true);
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            flags = flags.device_memory(true);
        }

        let frames = memory::allocate_frames_by_bytes_at(address, size).unwrap();
        let mapped_pages = kernel_mmi_ref
            .lock()
            .page_table
            .map_allocated_pages_to(pages, frames, flags)
            .unwrap();

        Self {
            inner: mapped_pages
                .into_borrowed_slice_mut(0, dimensions.height * dimensions.stride)
                .unwrap(),
            dimensions,
        }
    }

    #[inline]
    pub const fn dimensions(&self) -> FramebufferDimensions {
        self.dimensions
    }

    #[inline]
    pub const fn width(&self) -> usize {
        self.dimensions.width
    }

    #[inline]
    pub const fn height(&self) -> usize {
        self.dimensions.height
    }

    #[inline]
    pub const fn stride(&self) -> usize {
        self.dimensions.stride
    }

    #[inline]
    pub fn rows(&self) -> impl Iterator<Item = &[P]> {
        self.inner.chunks(self.stride())
    }

    #[inline]
    pub fn rows_mut(&mut self) -> impl Iterator<Item = &mut [P]> {
        let stride = self.stride();
        self.inner.chunks_mut(stride)
    }

    #[inline]
    pub fn contains<T>(&self, containable: T) -> bool
    where
        T: Containable,
    {
        // TODO: Width or stride?
        // TODO: Zero-width or zero-height framebuffer would panic.
        let rectangle = Rectangle::new(Coordinates::ZERO, self.width(), self.height());
        rectangle.contains(containable)
    }

    pub fn set(&mut self, coordinates: Coordinates, pixel: P) {
        let stride = self.stride();
        self[coordinates.y * stride + coordinates.x] = pixel;
    }
}

impl<P> Borrow<[P]> for Framebuffer<P>
where
    P: Pixel,
{
    fn borrow(&self) -> &[P] {
        self.deref()
    }
}

impl<P> BorrowMut<[P]> for Framebuffer<P>
where
    P: Pixel,
{
    fn borrow_mut(&mut self) -> &mut [P] {
        self.deref_mut()
    }
}

impl<P> Deref for Framebuffer<P>
where
    P: Pixel,
{
    type Target = [P];

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<P> DerefMut for Framebuffer<P>
where
    P: Pixel,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

mod private {
    pub trait Sealed {}
}
