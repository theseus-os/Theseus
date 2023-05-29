pub use core::alloc::AllocError;

use core::fmt;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct MappingError;

impl core::error::Error for MappingError {}

impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("creating memory mapping failed")
    }
}
