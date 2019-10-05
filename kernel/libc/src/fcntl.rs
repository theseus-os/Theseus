//! Taken from Relibc

use crate::types::*;

pub const O_RDONLY: c_int = 0x0001_0000;
pub const O_WRONLY: c_int = 0x0002_0000;
pub const O_RDWR: c_int = 0x0003_0000;
pub const O_ACCMODE: c_int = 0x0003_0000;
pub const O_NONBLOCK: c_int = 0x0004_0000;
pub const O_APPEND: c_int = 0x0008_0000;
pub const O_SHLOCK: c_int = 0x0010_0000;
pub const O_EXLOCK: c_int = 0x0020_0000;
pub const O_ASYNC: c_int = 0x0040_0000;
pub const O_FSYNC: c_int = 0x0080_0000;
pub const O_CLOEXEC: c_int = 0x0100_0000;
pub const O_CREAT: c_int = 0x0200_0000;
pub const O_TRUNC: c_int = 0x0400_0000;
pub const O_EXCL: c_int = 0x0800_0000;
pub const O_DIRECTORY: c_int = 0x1000_0000;
pub const O_PATH: c_int = 0x2000_0000;
pub const O_SYMLINK: c_int = 0x4000_0000;
// Negative to allow it to be used as int
pub const O_NOFOLLOW: c_int = -0x8000_0000;

pub const FD_CLOEXEC: c_int = 0x0100_0000;