use crate::Condvar;

/// A writer-preferred reader-writer lock.
///
/// The implementation is based on the sgx and hermit implementations of
/// reader-writer locks in `std::sys`.
#[derive(Debug, Default)]
pub struct RwLock {
    state: spin::Mutex<State>,
    readers: Condvar,
    writers: Condvar,
}

impl RwLock {
    // TODO: Make const.
    pub fn new() -> Self {
        Self {
            state: spin::Mutex::new(State::Unlocked),
            readers: Condvar::new(),
            writers: Condvar::new(),
        }
    }

    pub fn read(&self) {
        let mut state = self.state.lock();
        while !state.inc_readers() {
            // SAFETY: state corresponds to self.state.
            state = unsafe { self.readers.wait_spin(&self.state, state) };
        }
    }

    pub fn try_read(&self) -> bool {
        let mut state = self.state.lock();
        state.inc_readers()
    }

    pub fn write(&self) {
        let mut state = self.state.lock();
        while !state.inc_writers() {
            // SAFETY: state corresponds to self.state.
            state = unsafe { self.readers.wait_spin(&self.state, state) };
        }
    }

    pub fn try_write(&self) -> bool {
        let mut state = self.state.lock();
        state.inc_writers()
    }

    /// Unlocks previously acquired shared access to this lock.
    ///
    /// # Safety
    ///
    /// Behavior is undefined if the current thread does not have shared access.
    pub unsafe fn read_unlock(&self) {
        let mut state = self.state.lock();
        // if we were the last reader
        if state.dec_readers() {
            self.writers.notify_one();
        }
    }

    /// Unlocks previously acquired exclusive access to this lock.
    ///
    /// # Safety
    ///
    /// Behavior is undefined if the current thread does not have exclusive
    /// access.
    pub unsafe fn write_unlock(&self) {
        let mut state = self.state.lock();
        state.dec_writers();
        // if no writers were waiting for the lock
        if !self.writers.notify_one() {
            self.readers.notify_all();
        }
    }
}

#[derive(Clone, Debug)]
enum State {
    Unlocked,
    Reading(usize),
    Writing,
}

impl Default for State {
    fn default() -> Self {
        Self::Unlocked
    }
}

impl State {
    fn inc_readers(&mut self) -> bool {
        match *self {
            State::Unlocked => {
                *self = State::Reading(1);
                true
            }
            State::Reading(ref mut count) => {
                *count += 1;
                true
            }
            State::Writing => false,
        }
    }

    fn inc_writers(&mut self) -> bool {
        match *self {
            State::Unlocked => {
                *self = State::Writing;
                true
            }
            State::Reading(_) | State::Writing => false,
        }
    }

    fn dec_readers(&mut self) -> bool {
        let zero = match *self {
            State::Reading(ref mut count) => {
                *count -= 1;
                *count == 0
            }
            State::Unlocked | State::Writing => {
                panic!("attempted to decrement readers in non-reader state")
            }
        };
        if zero {
            *self = State::Unlocked;
        }
        zero
    }

    fn dec_writers(&mut self) {
        match *self {
            State::Writing => {}
            State::Unlocked | State::Reading(_) => {
                panic!("attempted to decrement writers in non-writer state")
            }
        }
        *self = State::Unlocked;
    }
}
