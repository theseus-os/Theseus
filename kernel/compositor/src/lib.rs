//! Stuff.
//!
//! Equivalent to a Wayland compositor.

#![no_std]
#![feature(negative_impls)]

extern crate alloc;

mod window;

use alloc::sync::Arc;
use core::{
    ops::Deref,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use async_channel::Channel;
use futures::StreamExt;
use graphics::GraphicsDriver;
pub use graphics::{AlphaPixel, Coordinates, Framebuffer, Pixel, Rectangle};
use hashbrown::HashMap;
use keyboard::KeyEvent;
use log::error;
use mouse::MouseEvent;
use window::LockedInner;

pub use crate::window::Window;

static COMPOSITOR: Option<Channel<Request>> = None;

static IS_INITIALISED: AtomicBool = AtomicBool::new(false);

pub fn init<T>(driver: T) -> Result<Channels, &'static str>
where
    T: Into<GraphicsDriver<AlphaPixel>>,
{
    // TODO: Orderings??
    if IS_INITIALISED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .unwrap_or(false)
    {
        let channels = Channels::new();
        let cloned = channels.clone();
        dreadnought::task::spawn_async(compositor_loop(driver.into(), cloned))?;
        Ok(channels)
    } else {
        panic!("initialised compositor multiple times");
    }
}

pub fn screen_size() -> (usize, usize) {
    // TODO
    (1920, 1080)
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
    T: Deref<Target = LockedInner>,
{
    let framebuffer = driver.back();

    // TODO: Take into account windows above.
    // TODO: Take into account dirty rectangle.
    for (i, row) in window.framebuffer.rows().enumerate() {
        let start = (window.coordinates.y + i) * framebuffer.stride();
        let end = start + row.len();
        framebuffer[start..end].clone_from_slice(row);
    }

    // TODO: This should be called in an interrupt handler or smthing like that to
    // prevent screen tearing.
    driver.swap(&[absolute(window.coordinates, dirty)]);
}

#[derive(Clone)]
pub struct Channels {
    // FIXME: Event type
    window: Channel<Request>,
    // FIXME: Deadlock prevention.
    keyboard: Channel<keyboard::KeyEvent>,
    // FIXME: Deadlock prevention.
    mouse: Channel<mouse::MouseEvent>,
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
// TODO: Optimisation: struct of arrays: RwLock<(Vec<usize>, Vec<Coordinates>,
// Vec<Framebuffer>, Vec<Channel>)> ordered by z-index. To get all rectangles of
// windows above window n, just do WINDOWS.read().2[n..].clone() to minimise
// holding the lock. Not sure about race conditions, and if we need to hold the
// lock the entire time.
// TODO: Should this be stored in the compositor?
static WINDOWS: Option<sync_spin::RwLock<HashMap<usize, Arc<window::Inner>>>> = None;
static WINDOW_ID: AtomicUsize = AtomicUsize::new(0);

pub fn window(width: usize, height: usize) -> Window {
    let (Window { id, inner }, clone) =
        Window::new(WINDOW_ID.fetch_add(1, Ordering::Relaxed), width, height);
    WINDOWS.as_ref().unwrap().write().insert(id, inner);
    clone
}

async fn compositor_loop(mut driver: GraphicsDriver<AlphaPixel>, mut channels: Channels) {
    loop {
        // The select macro is not available in no_std.
        futures::select_biased!(
            request = channels.window.next() => {
                let request = request.unwrap();
                handle_window_request(&mut driver, request).await;
            }
            request = channels.keyboard.next() => {
                let request = request.unwrap();
                handle_keyboard_request(request);
            }
            request = channels.mouse.next() => {
                let request = request.unwrap();
                handle_mouse_request(request);
            }
            complete => panic!("compositor loop exited"),
        );
    }
}

async fn handle_window_request(driver: &mut GraphicsDriver<AlphaPixel>, request: Request) {
    let id = request.window_id;

    let windows = WINDOWS.as_ref().unwrap().read();
    let inner = windows.get(&id).cloned();
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

fn handle_keyboard_request(_request: keyboard::KeyEvent) {
    todo!();
}

fn handle_mouse_request(_request: mouse::MouseEvent) {
    todo!();
}
