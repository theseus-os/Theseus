//! Stuff.
//!
//! Equivalent to a Wayland compositor.

#![no_std]

extern crate alloc;

mod window;

use alloc::{boxed::Box, sync::Arc};
use core::sync::atomic::{AtomicUsize, Ordering};

use async_channel::Channel;
use futures::StreamExt;
use hashbrown::HashMap;
use log::error;
use memory::{BorrowedSliceMappedPages, Mutable};
use sync_spin::Mutex;
pub use window::Window;
use zerocopy::FromBytes;

static COMPOSITOR: Option<Channel<Request>> = None;

static DRIVER: Mutex<Option<Box<dyn GraphicsDriver>>> = Mutex::new(None);

pub fn init<T>(driver: T) -> Result<Channels, &'static str>
where
    T: GraphicsDriver + 'static,
{
    let mut maybe_driver = DRIVER.lock();
    assert!(
        maybe_driver.is_none(),
        "initialised compositor multiple times"
    );
    *maybe_driver = Some(Box::new(driver));

    let channels = Channels::new();
    let cloned = channels.clone();

    dreadnought::task::spawn_async(compositor_loop(cloned))?;
    Ok(channels)
}

#[derive(Clone)]
pub struct Request {
    window_id: usize,
    ty: RequestType,
}

#[derive(Clone)]
enum RequestType {
    /// Request the compositor to refresh the given dirty rectangle.
    ///
    /// The lock on the window must not be held when the request is made, and
    /// the application must wait for a release event prior to obtaining the
    /// lock again.
    Refresh { dirty: Rectangle },
}

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

#[derive(Clone)]
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

#[derive(Clone)]
pub enum Event {
    /// The compositor released the framebuffer.
    Release,
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

pub trait GraphicsDriver: Send + Sync {
    fn back_mut(&mut self) -> &mut Framebuffer<AlphaPixel>;

    // fn swap(rectangles: &[Rectangle]);
    fn swap(&self);

    fn post_swap(&self);
}

fn redraw(window: &window::Inner, _dirty: Rectangle) {
    let mut locked = DRIVER.lock();
    let driver = locked.as_mut().unwrap();
    let framebuffer = driver.back_mut();

    // TODO: Take into account dirty rectangle.

    for (i, row) in window.framebuffer.rows().enumerate() {
        let start = (window.coordinates.y + i) * framebuffer.stride;
        let end = start + row.len();
        framebuffer.buffer[start..end].clone_from_slice(row);
    }

    driver.swap();
    driver.post_swap();
}

pub trait SingleBufferGraphicsDriver {
    fn write();
}

pub struct SimpleDriver {
    inner: Framebuffer<AlphaPixel>,
}

impl SimpleDriver {
    pub unsafe fn new(physical_address: usize, width: usize, height: usize) {
        todo!();
    }
}

impl GraphicsDriver for SimpleDriver {
    fn back_mut(&mut self) -> &mut Framebuffer<AlphaPixel> {
        &mut self.inner
    }

    fn swap(&self) {}

    fn post_swap(&self) {}
}

pub trait DoubleBufferGraphicsDriver {
    fn write();
    fn swap();
}

#[derive(Clone, FromBytes)]
pub struct AlphaPixel {}

impl private::Sealed for AlphaPixel {}

impl Pixel for AlphaPixel {}

#[derive(Clone)]
pub struct Channels {
    // FIXME: Event type
    window: Channel<Request>,
    // FIXME: Deadlock prevention.
    keyboard: Channel<event_types::Event>,
    // FIXME: Deadlock prevention.
    mouse: Channel<event_types::Event>,
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

// spin rwlock?
// TODO: Optimisation.
static WINDOWS: Option<sync_spin::RwLock<HashMap<usize, Arc<Mutex<window::Inner>>>>> = None;
static WINDOW_ID: AtomicUsize = AtomicUsize::new(0);

pub fn window() -> Window {
    let (Window { id, inner }, clone) = Window::new(WINDOW_ID.fetch_add(1, Ordering::Relaxed));
    WINDOWS.as_ref().unwrap().write().insert(id, inner);
    clone
}

async fn compositor_loop(mut channels: Channels) {
    loop {
        // The select macro is not available in no_std.
        futures::select_biased!(
            event = channels.window.next() => {
                let event = event.unwrap();
                handle_window_event(event);
            }
            event = channels.keyboard.next() => {
                let event = event.unwrap();
                handle_keyboard_event(event);
            }
            event = channels.mouse.next() => {
                let event = event.unwrap();
                handle_mouse_event(event);
            }
            // _ = fut => panic!("compositor loop exited"),
            default => panic!("ue"),
            complete => panic!("compositor loop exited"),
        );
    }
}

fn handle_window_event(event: Request) {
    let id = event.window_id;

    let windows = WINDOWS.as_ref().unwrap().read();
    let window = windows.get(&id).cloned();
    drop(windows);

    if let Some(window) = window {
        if let Some(inner) = window.try_lock() {
            match event.ty {
                RequestType::Refresh { dirty } => redraw(&inner, dirty),
            }
        } else {
            error!("window was locked");
        }
    } else {
        error!("invalid window ID");
    }
}

fn handle_keyboard_event(_event: event_types::Event) {
    todo!();
}

fn handle_mouse_event(_event: event_types::Event) {
    todo!();
}
