use alloc::{boxed::Box, sync::Arc};
use core::{
    cell::UnsafeCell,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use async_channel::Channel;
use graphics::{AlphaPixel, Coordinates, Framebuffer, FramebufferDimensions, Rectangle};
use sync_spin::{Mutex, MutexGuard};

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
    pub(crate) locked: UnsafeCell<LockedInner>,
    // TODO: Not spin. (unbounded?)
    pub(crate) events: Channel<Event>,
}

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
            locked: UnsafeCell::new(LockedInner {
                coordinates: Coordinates::ZERO,
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

    pub fn framebuffer(&mut self) -> impl DerefMut<Target = Framebuffer<AlphaPixel>> + '_ {
        struct Temp<'a> {
            inner: MutexGuard<'a, LockedInner>,
        }

        impl Deref for Temp<'_> {
            type Target = Framebuffer<AlphaPixel>;

            fn deref(&self) -> &Self::Target {
                &self.inner.framebuffer
            }
        }

        impl DerefMut for Temp<'_> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.inner.framebuffer
            }
        }

        Temp {
            inner: self.inner.locked.try_lock().unwrap(),
        }
    }

    pub fn area(&self) -> Rectangle {
        todo!();
    }

    pub fn handle_event(&self) -> Result<Option<event_types::Event>, &'static str> {
        todo!();
    }

    pub fn blocking_refresh(&mut self, dirty: Rectangle) {
        let (waker, blocker) = waker::new_waker();
        self.inner.locked.try_lock().unwrap().waker = Some(waker);
        COMPOSITOR.as_ref().unwrap().blocking_send(Request {
            window_id: self.id,
            ty: RequestType::Refresh { dirty },
        });
        blocker.block();
    }

    // Note that Refresh<'_> is tied to a mutable reference, and so no other mutable
    // accesses of self can occur until the future is consumed.
    pub async fn refresh(&mut self, dirty: Rectangle) -> Refresh<'_> {
        Refresh {
            locked: &self.inner.locked,
            id: self.id,
            dirty,
            state: State::Init,
        }
    }

    pub async fn event(&self) {
        todo!();
    }
}

pub struct Refresh<'a> {
    locked: &'a Mutex<LockedInner>,
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
            let mut locked = self.locked.try_lock().unwrap();
            locked.waker = Some(cx.waker().clone());
            locked.is_unlocked = false;
            drop(locked);

            let mut future = Box::pin(COMPOSITOR.as_ref().unwrap().send(Request {
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

        if let Some(locked) = self.locked.try_lock() {
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
