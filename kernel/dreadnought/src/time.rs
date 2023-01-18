//! Utilities for tracking time.

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub use time::Duration;

/// Waits until the specified number of ticks has elapsed.
///
/// The end number of ticks is calculated when the function is called so:
/// ```norun
/// # async fn main() {
/// let future = dreadnought::time::sleep(1000);
/// // Blocking sleep
/// sleep::sleep(1000);
/// future.await;
/// # }
/// ```
/// Would take 1000 ticks to complete.
pub fn sleep(duration: Duration) -> Sleep {
    let current_time = time::now::<time::Monotonic>();
    let until = current_time + duration;
    Sleep { until }
}

/// Future returned by [`sleep`].
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Sleep {
    until: time::Instant,
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        sleep::future::sleep_until(self.until, context.waker())
    }
}

// TODO: We should remove the waker from the sleep task list when Sleep is
// dropped. Currently it'll lead to a spurious wakeup of the task at some point
// in the future.
