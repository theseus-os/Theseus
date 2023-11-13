use alloc::{boxed::Box, sync::Arc};
use core::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use async_channel::Channel;
use graphics::{AlphaPixel, Coordinates, Framebuffer, FramebufferDimensions, Rectangle};
use sync_spin::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{Event, Request, RequestType, COMPOSITOR};

pub struct Window {
    pub(crate) id: usize,
    /// The data contained in the window.
    ///
    /// We use a spin mutex, because this mutex should never experience
    /// contention. The application and compositor coordinate access using
    /// messages. Strictly speaking, the mutex isn't even necessary.
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    // TODO: This could be an `UnsafeCell<LockedInner>`.
    pub(crate) locked: RwLock<LockedInner>,
    // TODO: Not spin. (unbounded?)
    pub(crate) events: Channel<Event>,
}

#[derive(Debug)]
pub(crate) struct LockedInner {
    pub(crate) coordinates: Coordinates,
    // pub border_size: usize,
    // pub title_bar_height: usize,
    pub(crate) framebuffer: Framebuffer<AlphaPixel>,
    pub(crate) waker: Option<Waker>,
    pub(crate) is_unlocked: bool,
}

// Explain why
unsafe impl Sync for Inner {}

// Functions that access inner.locked must take a mutable reference. This
// ensures that only one client thread is accessing inner.locked at any one
// time. The only other entity that could access inner.locked is the compositor,
// but compositor access is forcibly synchronised through the refresh and
// blocking_refresh methods.
impl Window {
    pub(crate) fn new(id: usize, width: usize, height: usize) -> (Self, Self) {
        let inner = Arc::new(Inner {
            locked: RwLock::new(LockedInner {
                coordinates: Coordinates::ORIGIN,
                framebuffer: Framebuffer::new_software(FramebufferDimensions {
                    width,
                    height,
                    stride: width,
                }),
                waker: None,
                is_unlocked: false,
            }),
            events: Channel::new(16),
        });
        (
            Self {
                id,
                inner: inner.clone(),
            },
            Self { id, inner },
        )
    }

    pub fn as_framebuffer(&self) -> impl Deref<Target = Framebuffer<AlphaPixel>> + '_ {
        FramebufferRef {
            inner: self.inner.locked.try_read().unwrap(),
        }
    }

    pub fn as_mut_framebuffer(&mut self) -> impl DerefMut<Target = Framebuffer<AlphaPixel>> + '_ {
        FramebufferMutRef {
            inner: self.inner.locked.try_write().unwrap(),
        }
    }

    pub fn area(&self) -> Rectangle {
        Rectangle::new(Coordinates::ORIGIN, 0x500, 0x400)
    }

    pub async fn recv(&self) -> Event {
        self.inner.events.recv().await
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.inner.events.try_recv()
    }

    pub fn blocking_recv(&self) -> Event {
        self.inner.events.blocking_recv()
    }

    // IDEA(tsoutsman):
    //
    // As it currently stands, the client must wait for the compositor to release
    // the buffer before doing anything else. This isn't too bad, as we don't have
    // graphics-intesive applications, but it's something that could be improved on.
    //
    // `refresh` could be done in a two-step process, returning a future that itself
    // returns a future. The first future would send the request, and the second
    // future would wait for a response. Notably, the second future would still
    // be tied to a mutable reference to self. This is both a blessing and a
    // curse, because it correctly enforces that the window can't be used until
    // both futures are resolved, but I'm pretty sure the borrow checker would
    // have a fit if the second future was stored in a variable across a loop
    // (i.e. event loop) iteration. However, this could probably be worked around
    // with some trickery.
    //
    // One could imaging a double-buffering client that commits a front buffer (by
    // awaiting the first future), and stores the second future. It then starts
    // calculating the next frame and drawing it into the back buffer. After it's
    // done, it polls the second future (checking if the server has released the
    // shared buffer), and if it's ready, switches the back buffer into the shared
    // buffer.
    //
    // A similar thing could be done with `blocking_refresh`.

    // Note that Refresh<'_> is tied to a mutable reference, and so no other mutable
    // accesses of self can occur until the future is consumed.
    pub fn refresh(&mut self, dirty: Rectangle) -> Refresh<'_> {
        Refresh {
            locked: &self.inner.locked,
            id: self.id,
            dirty,
            state: State::Init,
        }
    }

    pub fn blocking_refresh(&mut self, dirty: Rectangle) {
        let (waker, blocker) = waker::new_waker();
        self.inner.locked.try_write().unwrap().waker = Some(waker);
        COMPOSITOR.get().unwrap().blocking_send(Request {
            window_id: self.id,
            ty: RequestType::Refresh { dirty },
        });
        blocker.block();
    }

    pub async fn event(&self) {
        todo!();
    }
}

pub struct Refresh<'a> {
    locked: &'a RwLock<LockedInner>,
    id: usize,
    dirty: Rectangle,
    state: State,
}

enum State {
    Init,
    Sending(Pin<Box<dyn Future<Output = ()>>>),
    Sent,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }
}

impl<'a> Future for Refresh<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Manually implementing async functions :)

        if self.state == State::Init {
            let mut locked = self.locked.try_write().unwrap();
            locked.waker = Some(cx.waker().clone());
            locked.is_unlocked = false;
            drop(locked);

            let mut future = Box::pin(COMPOSITOR.get().unwrap().send(Request {
                window_id: self.id,
                ty: RequestType::Refresh { dirty: self.dirty },
            }));
            let output = Pin::new(&mut future).poll(cx);

            match output {
                Poll::Ready(()) => self.state = State::Sent,
                Poll::Pending => {
                    self.state = State::Sending(future);
                    return Poll::Pending;
                }
            }
        } else if let State::Sending(ref mut future) = self.state {
            match Pin::new(future).poll(cx) {
                Poll::Ready(()) => self.state = State::Sent,
                Poll::Pending => return Poll::Pending,
            };
        }

        // State::Sent

        if let Some(locked) = self.locked.try_read() {
            if locked.is_unlocked {
                return Poll::Ready(());
            }
        }

        // This is not a race condition. Even if the compositor unlocks between our
        // check and returning Poll::Pending, it would have woken the waker,
        // guaranteeing that this future will be polled again.
        Poll::Pending
    }
}

struct FramebufferRef<'a> {
    inner: RwLockReadGuard<'a, LockedInner>,
}

impl Deref for FramebufferRef<'_> {
    type Target = Framebuffer<AlphaPixel>;

    fn deref(&self) -> &Self::Target {
        &self.inner.framebuffer
    }
}

struct FramebufferMutRef<'a> {
    inner: RwLockWriteGuard<'a, LockedInner>,
}

impl Deref for FramebufferMutRef<'_> {
    type Target = Framebuffer<AlphaPixel>;

    fn deref(&self) -> &Self::Target {
        &self.inner.framebuffer
    }
}

impl DerefMut for FramebufferMutRef<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner.framebuffer
    }
}
