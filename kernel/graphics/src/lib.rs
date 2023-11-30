#![no_std]
#![feature(const_trait_impl)]

mod framebuffer;
mod pixel;

use core::cmp::min;

pub use geometry::{Coordinates, Horizontal, Rectangle, Vertical};

pub use crate::{
    framebuffer::{Framebuffer, FramebufferDimensions},
    pixel::{AlphaPixel, Pixel},
};

pub struct SoftwareDoubleBuffer<P>
where
    P: Pixel,
{
    front: Framebuffer<P>,
    back: Framebuffer<P>,
}

impl<P> SoftwareDoubleBuffer<P>
where
    P: Pixel,
{
    /// Returns a double buffered software driver given a hardware buffer
    /// `front`.
    pub fn new(front: Framebuffer<P>) -> Self {
        Self {
            back: Framebuffer::new_software(front.dimensions()),
            front,
        }
    }

    pub fn back(&mut self) -> &mut Framebuffer<P> {
        &mut self.back
    }

    pub fn swap(&mut self, rectangles: &[Rectangle]) {
        let start = time::Instant::now();
        for rectangle in rectangles {
            let top = rectangle.y(Vertical::Top);
            // Non-inclusive
            let bottom = min(top + rectangle.height(), self.height());

            let left = rectangle.x(Horizontal::Left);
            // Non-inclusive
            // TODO: Width or stride?
            let right = min(left + rectangle.width(), self.width());

            // log::error!("{rectangle:?}");
            // log::error!("{top:?} {bottom:?} {left:?} {right:?}");

            if left == 0
                && right == self.stride() - 1
                // TODO: Do we need this condition?
                && self.width() == self.stride()
            {
                let start = top * self.stride();
                let end = bottom * self.stride();

                self.front[start..end].copy_from_slice(&self.back[start..end]);
            }

            let num_rows = bottom - top;

            for (front_row, back_row) in self
                .front
                .rows_mut()
                .skip(top)
                .take(num_rows)
                .zip(self.back.rows_mut().skip(top).take(num_rows))
            {
                front_row[left..right].copy_from_slice(&back_row[left..right]);
            }
        }
        // log::warn!("thingy took: {:?}", time::Instant::now().duration_since(start));
        // log::warn!("rectangle length: {}", rectangles.len());
    }

    // The front and back buffers have the same dimension.

    #[inline]
    pub fn width(&self) -> usize {
        self.front.width()
    }

    #[inline]
    pub fn height(&self) -> usize {
        self.front.height()
    }

    #[inline]
    pub fn stride(&self) -> usize {
        self.front.stride()
    }
}

pub struct HardwareDoubleBuffer<P>
where
    P: Pixel,
{
    front: Framebuffer<P>,
    back: Framebuffer<P>,
    swap_function: fn(),
}

impl<P> HardwareDoubleBuffer<P>
where
    P: Pixel,
{
    pub fn new(front: Framebuffer<P>, back: Framebuffer<P>, swap_function: fn()) -> Self {
        Self {
            front,
            back,
            swap_function,
        }
    }

    pub fn back(&mut self) -> &mut Framebuffer<P> {
        &mut self.back
    }

    pub fn swap(&mut self) {
        // TODO: This is correct right?
        core::mem::swap(&mut self.front, &mut self.back);
        (self.swap_function)();
    }
}

// pub struct HardwareTripleBuffer<P>
// where
//     P: Pixel,
// {
//     front: Framebuffer<P>,
//     first_back: Framebuffer<P>,
//     second_back: Framebuffer<P>,
// }

pub enum GraphicsDriver<P>
where
    P: Pixel,
{
    SoftwareDouble(SoftwareDoubleBuffer<P>),
    HardwareDouble(HardwareDoubleBuffer<P>),
    // HardwareTriple(HardwareTripleBuffer<P>),
}

impl<P> GraphicsDriver<P>
where
    P: Pixel,
{
    pub fn back(&mut self) -> &mut Framebuffer<P> {
        match self {
            GraphicsDriver::SoftwareDouble(b) => b.back(),
            GraphicsDriver::HardwareDouble(b) => b.back(),
        }
    }

    pub fn swap(&mut self, rectangles: &[Rectangle]) {
        match self {
            // TODO: Compute rectangle overlap.
            GraphicsDriver::SoftwareDouble(b) => b.swap(rectangles),
            GraphicsDriver::HardwareDouble(b) => b.swap(),
        }
    }
}

impl<P> From<SoftwareDoubleBuffer<P>> for GraphicsDriver<P>
where
    P: Pixel,
{
    fn from(value: SoftwareDoubleBuffer<P>) -> Self {
        Self::SoftwareDouble(value)
    }
}

impl<P> From<HardwareDoubleBuffer<P>> for GraphicsDriver<P>
where
    P: Pixel,
{
    fn from(value: HardwareDoubleBuffer<P>) -> Self {
        Self::HardwareDouble(value)
    }
}

// impl<P> From<HardwareTripleBuffer<P>> for GraphicsDriver<P>
// where
//     P: Pixel,
// {
//     fn from(value: HardwareTripleBuffer<P>) -> Self {
//         Self::HardwareTriple(value)
//     }
// }
