use alloc::{boxed::Box, sync::Arc, task::Wake};
use core::pin::Pin;
use futures::Future;

pub struct Task {
    // TODO: This should use a SyncUnsafeCell rather than a Mutex but mpmc (and by extension
    // async_channel) have incorrect bounds on their sync implementation. The senders should be
    // sync if T is send; whether T is sync or not doesn't matter.
    pub(super) future: core::cell::SyncUnsafeCell<Pin<Box<dyn Future<Output = ()> + 'static + Send>>>,
    // pub(super) future: spin::Mutex<Pin<Box<dyn Future<Output = ()> + 'static + Send>>>,
    // TODO: Use an mpsc rather than an mpmc.
    pub(super) run_queue: async_channel::Sender<Task>,
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        let _ = self.run_queue.send(self.clone());
    }
}

pub fn is_send<T: Send>() {}
pub fn is_sync<T: Send>() {}

pub fn test() {
    // is_send::<Task>();
}
