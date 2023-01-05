#![no_std]

pub fn join(task: task::JoinableTaskRef) -> Result<task::ExitValue, &'static str> {
    let (waker, blocker) = waker::waker();
    task.set_waker(waker);
    if !task.has_exited() {
        blocker.block();
    }
    task.join()
}
