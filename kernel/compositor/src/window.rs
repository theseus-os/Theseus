use alloc::sync::Arc;

use async_channel::Channel;
use sync_spin::Mutex;

use crate::{
    AlphaPixel, Coordinates, Event, Framebuffer, Rectangle, Request, RequestType, COMPOSITOR,
};

pub struct Window {
    pub(crate) id: usize,
    /// The data contained in the window.
    ///
    /// We use a spin mutex, because this mutex should never experience
    /// contention. The application and compositor coordinate access using
    /// messages. Strictly speaking, the mutex isn't even necessary.
    pub(crate) inner: Arc<Mutex<Inner>>,
}

pub(crate) struct Inner {
    pub(crate) coordinates: Coordinates,
    // pub border_size: usize,
    // pub title_bar_height: usize,
    pub(crate) framebuffer: Framebuffer<AlphaPixel>,
    events: Channel<Event>,
}

impl Window {
    // pub fn framebuffer(&self) -> impl Deref<Target = Framebuffer<AlphaPixel>> {
    //     self.inner.try_lock().unwrap().framebuffer
    // }

    // pub fn framebuffer_mut(&mut self) -> impl DerefMut<Target =
    // Framebuffer<AlphaPixel>> {     self.inner.try_lock().unwrap().framebuffer
    // }

    pub(crate) fn new(id: usize) -> (Self, Self) {
        todo!();
    }

    pub fn blocking_refresh(&self, dirty: Rectangle) {
        COMPOSITOR.as_ref().unwrap().blocking_send(Request {
            window_id: self.id,
            ty: RequestType::Refresh { dirty },
        });
        todo!("wait for response (or register callback?)");
    }

    pub async fn commit(&self, dirty: Rectangle) {
        COMPOSITOR
            .as_ref()
            .unwrap()
            .send(Request {
                window_id: self.id,
                ty: RequestType::Refresh { dirty },
            })
            .await;
        todo!("wait for response (or register callback?)");
    }

    pub async fn event(&self) {
        todo!();
    }
}
