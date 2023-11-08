#![no_std]

mod framebuffer;
mod pixel;
mod rectangle;

pub use framebuffer::Framebuffer;
pub use pixel::{AlphaPixel, Pixel};
pub use rectangle::{Coordinates, Rectangle};

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
        for rectangle in rectangles {
            for row in 0..rectangle.height {
                let x = rectangle.coordinates.x;
                let y = rectangle.coordinates.y + row;

                // front and back have the same stride.
                let start = y * self.front.stride() + x;
                let end = start + rectangle.width;

                // TODO: Return error.
                assert!(self.front.width() >= x + rectangle.width);

                self.front.inner[start..end].copy_from_slice(&self.back.inner[start..end]);
            }
        }
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
