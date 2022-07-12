pub trait Timer {
    fn value() -> Timespec;

    // TODO: enable, period/frequency, and accuracy
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timespec {
    pub secs: u64,
    pub nanos: u32,
}

impl Timespec {
    pub fn zero() -> Self {
        Self {
            secs: 0,
            nanos: 0,
        }
    }
}
