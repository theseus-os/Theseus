pub use bare_io::*;

pub fn last_os_error() -> Error {
    let errno = unsafe { crate::errno::errno };
    Error::from_raw_os_error(errno)
}
