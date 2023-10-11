//! Stuff.
//!
//! Equivalent to a Wayland compositor.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use async_channel::Channel;
use event_types::Event;
use futures::StreamExt;
use memory::{BorrowedSliceMappedPages, Mutable};
use zerocopy::FromBytes;

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

pub fn init() -> Result<Channels, &'static str> {
    let channels = Channels::new();
    let cloned = channels.clone();
    dreadnought::task::spawn_async(compositor_loop(cloned))?;
    Ok(channels)
}

#[derive(Clone)]
pub struct Channels {
    // FIXME: Event type
    window: Channel<u8>,
    // FIXME: Deadlock prevention.
    keyboard: Channel<Event>,
    // FIXME: Deadlock prevention.
    mouse: Channel<Event>,
}

impl Channels {
    fn new() -> Self {
        Self {
            window: Channel::new(8),
            keyboard: Channel::new(8),
            mouse: Channel::new(8),
        }
    }
}

async fn compositor_loop(mut channels: Channels) {
    loop {
        // The select macro is not available on no-std.
        futures::select_biased!(
            event = channels.window.next() => {
                todo!();
            }
            event = channels.keyboard.next() => {
                todo!();
            }
            event = channels.mouse.next() => {
                todo!();
            }
            complete => panic!("compositor loop exited"),
        );
    }
}
