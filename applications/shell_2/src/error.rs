pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug)]
pub enum Error {
    ExitRequested,
    CurrentTaskUnavailable,
    Command(isize),
}

impl Error {
    pub(crate) fn is_fatal(&self) -> bool {
        use Error::*;

        match self {
            ExitRequested | CurrentTaskUnavailable => true,
            Command(_) => false,
        }
    }

    pub(crate) fn exit_code(&self) -> isize {
        use Error::*;

        match self {
            Command(e) => *e,
            _ => 1,
        }
    }
}
