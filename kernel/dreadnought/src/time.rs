use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub fn sleep(ticks: usize) -> Sleep {
    let current_ticks = sleep::get_current_time_in_ticks();
    let wakeup = current_ticks + ticks;
    Sleep { wakeup }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Sleep {
    wakeup: usize,
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        sleep::future::sleep_until(self.wakeup, context.waker())
    }
}

// TODO: We should remove the waker from the sleep task list when Sleep is
// dropped. Currently it'll lead to a spurious wakeup of the task at some point
// in the future.
