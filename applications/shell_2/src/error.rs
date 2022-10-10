use alloc::{borrow::ToOwned, string::String};

pub type Result<T> = core::result::Result<T, Error>;

// #[derive(Debug)]
// pub struct Error {
//     kind: ErrorKind,
//     msg: String,
// }

// impl Error {
//     pub(crate) fn new(kind: ErrorKind, msg: String) -> Self {
//         Self { kind, msg }
//     }

//     pub(crate) fn is_fatal(&self) -> bool {
//         self.kind.is_fatal()
//     }

//     pub(crate) fn kind(&self) -> ErrorKind {
//         self.kind
//     }

//     pub(crate) fn exit_code(&self) -> isize {
//         self.kind.exit_code()
//     }
// }

// impl From<ErrorKind> for Error {
//     fn from(kind: ErrorKind) -> Self {
//         match kind {
//             ErrorKind::CurrentTaskUnavailable => {
//                 Self::new(kind, "could not get shell task".to_owned())
//             }
//             _ => Self::new(kind, String::new()),
//         }
//     }
// }

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
