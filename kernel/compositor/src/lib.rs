//! Stuff.
//!
//! Equivalent to a Wayland compositor.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use async_channel::Channel;
use futures::StreamExt;
use zerocopy::FromBytes;

// FIXME

use memory::{BorrowedSliceMappedPages, Mutable};

trait Draw {
    fn display<P>(
        &mut self,
        // coordinate: Coord,
        framebuffer: &mut Framebuffer<P>,
        // ) -> Result<Rectangle, &'static str>
    ) -> Result<(), &'static str>
    where
        P: Pixel;

    fn size(&self) -> (usize, usize);

    fn set_size(&mut self, width: usize, height: usize);
}

#[derive(Clone, Copy, PartialEq, Debug, Hash)]
pub struct Coordinates {
    pub x: usize,
    pub y: usize,
}

pub struct Rectangle {
    pub coordinates: Coordinates,
    pub width: usize,
    pub height: usize,
}

pub struct Framebuffer<P>
where
    P: Pixel,
{
    buffer: BorrowedSliceMappedPages<P, Mutable>,
    stride: usize,
    width: usize,
    height: usize,
}

impl<P> Framebuffer<P>
where
    P: Pixel,
{
    pub fn rows(&self) -> impl Iterator<Item = &[P]> {
        self.buffer.chunks(self.stride)
    }

    pub fn rows_mut(&mut self) -> impl Iterator<Item = &mut [P]> {
        self.buffer.chunks_mut(self.stride)
    }
}

// TODO: Should it be sealed?
pub trait Pixel: private::Sealed + FromBytes {}

mod private {
    pub trait Sealed {}
}

trait GraphicsDriver {
    fn back_mut(&mut self) -> &mut Framebuffer<AlphaPixel>;

    // fn swap(rectangles: &[Rectangle]);
    fn swap();

    fn post_swap();
}

pub struct Window {
    coordinates: Coordinates,
    // pub border_size: usize,
    // pub title_bar_height: usize,
    // event_producer: Queue<Event>,
    framebuffer: Framebuffer<AlphaPixel>,
}

impl Window {
    pub fn framebuffer(&self) -> &Framebuffer<AlphaPixel> {
        &self.framebuffer
    }

    pub fn framebuffer_mut(&mut self) -> &mut Framebuffer<AlphaPixel> {
        &mut self.framebuffer
    }
}

fn draw() {
    let windows: Vec<Window> = Vec::new();
    for window in windows {}
}

pub trait SingleBufferGraphicsDriver {
    fn write();
}

pub trait DoubleBufferGraphicsDriver {
    fn write();
    fn swap();
}

#[derive(FromBytes)]
pub struct AlphaPixel {}

impl private::Sealed for AlphaPixel {}

impl Pixel for AlphaPixel {}

pub fn init() {
    todo!();
}

async fn compositor_loop() {
    let mut keyboard_events = Channel::<u8>::new(8);
    let mut mouse_events = Channel::<u8>::new(8);
    let mut window_events = Channel::<u8>::new(8);

    loop {
        // The select macro is not available on no-std.
        futures::select_biased!(
            event = window_events.next() => {
                todo!();
            }
            event = keyboard_events.next() => {
                todo!();
            }
            event = mouse_events.next() => {
                todo!();
            }
            complete => panic!("compositor loop exited"),
        );
    }
}
