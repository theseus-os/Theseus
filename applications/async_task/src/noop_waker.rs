use core::task::{RawWaker, RawWakerVTable, Waker};

pub fn waker() -> Waker {
    unsafe { Waker::from_raw(noop_waker()) }
}

fn noop_waker() -> RawWaker {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        noop_waker()
    }

    let vtable = &RawWakerVTable::new(clone, no_op, no_op, no_op);
    RawWaker::new(0 as *const (), vtable)
}
