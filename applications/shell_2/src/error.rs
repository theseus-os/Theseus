use alloc::string::String;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub enum Error {
    ExitRequested,
    CurrentTaskUnavailable,
    CommandNotFound(String),
    Command(isize),
}
