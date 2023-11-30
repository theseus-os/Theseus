//! This crate is responsible for composing window buffers, handling window
//! interactions (e.g. dragging, resizing), and sending input to the correct
//! window.
//!
//! It is roughly equivalent in scope to a compositing window manager on Linux.

#![no_std]
#![feature(negative_impls)]

extern crate alloc;

mod window;

use alloc::sync::Arc;
use core::{
    ops::Deref,
    sync::atomic::{AtomicUsize, Ordering},
};

use async_channel::Channel;
use futures::StreamExt;
use graphics::{GraphicsDriver, Horizontal, Vertical};
pub use graphics::{AlphaPixel, Coordinates, Framebuffer, Pixel, Rectangle};
use hashbrown::HashMap;
use keyboard::KeyEvent;
use log::error;
use mouse::MouseEvent;
use spin::Once;
use sync_spin::RwLock;
use window::LockedInner;

use crate::window::Inner;
pub use crate::window::Window;

static COMPOSITOR: Once<Channel<Request>> = Once::new();

pub fn init<T>(driver: T) -> Result<Channels, &'static str>
where
    T: Into<GraphicsDriver<AlphaPixel>>,
{
    let mut windows = WINDOWS.write();
    if windows.is_some() {
        return Err("initialised compositor multiple times");
    }
    *windows = Some(HashMap::new());

    let channels = Channels::new();
    let cloned = channels.clone();
    // TODO: Remove clone.
    COMPOSITOR.call_once(|| channels.window.clone());

    dreadnought::task::spawn_async(compositor_loop(driver.into(), cloned))?;
    Ok(channels)
}

pub fn screen_size() -> (usize, usize) {
    // TODO
    (0x500, 0x400)
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

#[derive(Clone, Debug)]
pub enum Event {
    Keyboard(KeyEvent),
    Mouse(MouseEvent),
    Resize(Rectangle),
}

fn absolute(coordinates: Coordinates, mut rectangle: Rectangle) -> Rectangle {
    rectangle.coordinates += coordinates;
    rectangle
}

fn refresh<T>(driver: &mut GraphicsDriver<AlphaPixel>, window: T, dirty: Rectangle)
where
    T: Deref<Target = LockedInner> + core::fmt::Debug,
{
    let framebuffer = driver.back();

    // TODO: Take into account windows above.
    // TODO: Take into account dirty rectangle.
    let left = dirty.x(Horizontal::Left);
    let width = dirty.width();

    for y in dirty.y(Vertical::Top)..(dirty.y(Vertical::Bottom) + 1) {
        let start = y * framebuffer.stride() + left;
        let end = start + width;
        framebuffer[start..end].clone_from_slice(&window.framebuffer[start..end]);
    }

    // log::warn!("a: {:?}", time::Instant::now().duration_since(start));
    // TODO: This should be called in an interrupt handler or something like that to
    //  prevent screen tearing.
    driver.swap(&[absolute(window.coordinates, dirty)]);
}

#[derive(Clone)]
pub struct Channels {
    // FIXME: Deadlock prevention.
    pub window: Channel<Request>,
    // FIXME: Deadlock prevention.
    pub keyboard: Channel<KeyEvent>,
    // FIXME: Deadlock prevention.
    pub mouse: Channel<MouseEvent>,
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
// IDEA(tsoutsman): Optimisation: struct of arrays: RwLock<(Vec<usize>,
// Vec<Coordinates>), Vec<Framebuffer>, Vec<Channel>)> ordered by z-index. To
// get all rectangles of windows above window n, just do
// WINDOWS.read().2[n..].clone() to minimise holding the lock. Not sure about
// race conditions, and if we need to hold the lock the entire time.
static WINDOWS: RwLock<Option<HashMap<usize, Arc<Inner>>>> = RwLock::new(None);
static WINDOW_ID: AtomicUsize = AtomicUsize::new(0);

pub fn window(width: usize, height: usize) -> Window {
    let (Window { id, inner }, clone) =
        Window::new(WINDOW_ID.fetch_add(1, Ordering::Relaxed), width, height);
    WINDOWS.write().as_mut().unwrap().insert(id, inner);
    clone
}

async fn compositor_loop(mut driver: GraphicsDriver<AlphaPixel>, mut channels: Channels) {
    loop {
        // log::info!("compositor looping");
        // The select macro is not available in no_std.
        futures::select_biased!(
            request = channels.window.next() => {
                let start = time::Instant::now();
                let request = request.unwrap();
                handle_window_request(&mut driver, request).await;
            }
            request = channels.keyboard.next() => {
                let start = time::Instant::now();
                let request = request.unwrap();
                handle_keyboard_request(request);
            }
            request = channels.mouse.next() => {
                let request = request.unwrap();
                handle_mouse_request(request);
            }
            complete => panic!("compositor loop exited"),
        );
        // log::info!("compositor looped");
    }
}

async fn handle_window_request(driver: &mut GraphicsDriver<AlphaPixel>, request: Request) {
    let id = request.window_id;

    let windows = WINDOWS.read();
    let inner = windows.as_ref().unwrap().get(&id).cloned();
    drop(windows);

    // TODO: Take events out of inner (or at least out of Mutex).
    let mut waker = None;

    if let Some(inner) = inner {
        if let Some(mut locked) = inner.locked.try_write() {
            match request.ty {
                RequestType::Refresh { dirty } => {
                    // This will be true once we drop the lock.
                    locked.is_unlocked = true;

                    match &locked.waker {
                        Some(w) => waker = Some(w.clone()),
                        None => error!("no registered waker"),
                    }

                    refresh(driver, locked, dirty);
                }
            }
        } else {
            error!("window was locked");
        }
    } else {
        error!("invalid window ID");
    }

    if let Some(waker) = waker {
        waker.wake();
    }
}

fn handle_keyboard_request(event: KeyEvent) {
    if let Some(active_window) = active_window() {
        if active_window
            .events
            .try_send(Event::Keyboard(event))
            .is_err()
        {
            log::info!("dropping keyboard event");
        }
    }
}

fn handle_mouse_request(event: MouseEvent) {
    if let Some(active_window) = active_window() {
        if active_window.events.try_send(Event::Mouse(event)).is_err() {
            log::info!("dropping mouse event");
        }
    }
}

fn active_window() -> Option<Arc<Inner>> {
    // TODO: Get active window.
    WINDOWS.read().as_ref().unwrap().get(&0).cloned()
}
