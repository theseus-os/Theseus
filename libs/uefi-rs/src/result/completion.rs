use super::Status;
use log::warn;

/// This type is used when an UEFI operation has completed, but some non-fatal
/// problems may have been encountered along the way
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Completion<T> {
    status: Status,
    result: T,
}

impl<T> Completion<T> {
    /// Build a completion from a non-warning status and a function result
    pub fn new(status: Status, result: T) -> Self {
        Self { status, result }
    }

    /// Extract the status of this completion
    pub fn status(&self) -> Status {
        self.status
    }

    /// Split this completion into its inner status and result data
    pub fn split(self) -> (Status, T) {
        (self.status, self.result)
    }

    /// Access the inner value, logging the warning if there is any
    pub fn log(self) -> T {
        if self.status != Status::SUCCESS {
            log_warning(self.status);
        }
        self.result
    }

    /// Assume that no warning occured, panic if not
    pub fn unwrap(self) -> T {
        if self.status != Status::SUCCESS {
            unwrap_failed(
                "Called `Completion::unwrap()` on a `Warning` value",
                self.status,
            );
        }
        self.result
    }

    /// Assume that no warning occured, panic with provided message if not
    pub fn expect(self, msg: &str) -> T {
        if self.status != Status::SUCCESS {
            unwrap_failed(msg, self.status);
        }
        self.result
    }

    /// Transform the inner value without unwrapping the Completion
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Completion<U> {
        Completion {
            status: self.status,
            result: f(self.result),
        }
    }

    /// Merge this completion with a success or warning status
    ///
    /// Since this type only has storage for one warning, if two warnings must
    /// be stored, one of them will be spilled into the logs.
    pub fn with_status(self, extra_status: Status) -> Self {
        if extra_status.is_success() {
            self
        } else {
            Completion {
                status: extra_status,
                result: self.log(),
            }
        }
    }
}

// Completions can be built from either a status or a payload

impl From<Status> for Completion<()> {
    fn from(status: Status) -> Self {
        Completion { status, result: () }
    }
}

impl<T> From<T> for Completion<T> {
    fn from(result: T) -> Self {
        Completion {
            status: Status::SUCCESS,
            result,
        }
    }
}

// These are separate functions to reduce the code size of the methods

#[inline(never)]
#[cold]
fn unwrap_failed(msg: &str, warning: Status) -> ! {
    panic!("{}: {:?}", msg, warning)
}

#[inline(never)]
#[cold]
fn log_warning(warning: Status) {
    warn!("Encountered UEFI warning: {:?}", warning)
}
