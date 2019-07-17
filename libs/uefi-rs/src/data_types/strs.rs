use super::chars::{Char16, Char8, NUL_16, NUL_8};
use core::convert::TryInto;
use core::result::Result;
use core::slice;

/// Errors which can occur during checked [uN] -> CStrN conversions
pub enum FromSliceWithNulError {
    /// An invalid character was encountered before the end of the slice
    InvalidChar(usize),

    /// A null character was encountered before the end of the slice
    InteriorNul(usize),

    /// The slice was not null-terminated
    NotNulTerminated,
}

/// A Latin-1 null-terminated string
///
/// This type is largely inspired by `std::ffi::CStr`, see the documentation of
/// `CStr` for more details on its semantics.
#[repr(transparent)]
pub struct CStr8([Char8]);

impl CStr8 {
    /// Wraps a raw UEFI string with a safe C string wrapper
    pub unsafe fn from_ptr<'ptr>(ptr: *const Char8) -> &'ptr Self {
        let mut len = 0;
        while *ptr.add(len) != NUL_8 {
            len += 1
        }
        let ptr = ptr as *const u8;
        Self::from_bytes_with_nul_unchecked(slice::from_raw_parts(ptr, len + 1))
    }

    /// Creates a C string wrapper from bytes
    pub fn from_bytes_with_nul(chars: &[u8]) -> Result<&Self, FromSliceWithNulError> {
        let nul_pos = chars.iter().position(|&c| c == 0);
        if let Some(nul_pos) = nul_pos {
            if nul_pos + 1 != chars.len() {
                return Err(FromSliceWithNulError::InteriorNul(nul_pos));
            }
            Ok(unsafe { Self::from_bytes_with_nul_unchecked(chars) })
        } else {
            Err(FromSliceWithNulError::NotNulTerminated)
        }
    }

    /// Unsafely creates a C string wrapper from bytes
    pub unsafe fn from_bytes_with_nul_unchecked(chars: &[u8]) -> &Self {
        &*(chars as *const [u8] as *const Self)
    }

    /// Returns the inner pointer to this C string
    pub fn as_ptr(&self) -> *const Char8 {
        self.0.as_ptr()
    }

    /// Converts this C string to a slice of bytes
    pub fn to_bytes(&self) -> &[u8] {
        let chars = self.to_bytes_with_nul();
        &chars[..chars.len() - 1]
    }

    /// Converts this C string to a slice of bytes containing the trailing 0 char
    pub fn to_bytes_with_nul(&self) -> &[u8] {
        unsafe { &*(&self.0 as *const [Char8] as *const [u8]) }
    }
}

/// An UCS-2 null-terminated string
///
/// This type is largely inspired by `std::ffi::CStr`, see the documentation of
/// `CStr` for more details on its semantics.
#[repr(transparent)]
pub struct CStr16([Char16]);

impl CStr16 {
    /// Wraps a raw UEFI string with a safe C string wrapper
    pub unsafe fn from_ptr<'ptr>(ptr: *const Char16) -> &'ptr Self {
        let mut len = 0;
        while *ptr.add(len) != NUL_16 {
            len += 1
        }
        let ptr = ptr as *const u16;
        Self::from_u16_with_nul_unchecked(slice::from_raw_parts(ptr, len + 1))
    }

    /// Creates a C string wrapper from a u16 slice
    ///
    /// Since not every u16 value is a valid UCS-2 code point, this function
    /// must do a bit more validity checking than CStr::from_bytes_with_nul
    pub fn from_u16_with_nul(codes: &[u16]) -> Result<&Self, FromSliceWithNulError> {
        for (pos, &code) in codes.iter().enumerate() {
            match code.try_into() {
                Ok(NUL_16) => {
                    if pos != codes.len() - 1 {
                        return Err(FromSliceWithNulError::InteriorNul(pos));
                    } else {
                        return Ok(unsafe { Self::from_u16_with_nul_unchecked(codes) });
                    }
                }
                Err(_) => {
                    return Err(FromSliceWithNulError::InvalidChar(pos));
                }
                _ => {}
            }
        }
        Err(FromSliceWithNulError::NotNulTerminated)
    }

    /// Unsafely creates a C string wrapper from a u16 slice.
    pub unsafe fn from_u16_with_nul_unchecked(codes: &[u16]) -> &Self {
        &*(codes as *const [u16] as *const Self)
    }

    /// Returns the inner pointer to this C string
    pub fn as_ptr(&self) -> *const Char16 {
        self.0.as_ptr()
    }

    /// Converts this C string to a u16 slice
    pub fn to_u16_slice(&self) -> &[u16] {
        let chars = self.to_u16_slice_with_nul();
        &chars[..chars.len() - 1]
    }

    /// Converts this C string to a u16 slice containing the trailing 0 char
    pub fn to_u16_slice_with_nul(&self) -> &[u16] {
        unsafe { &*(&self.0 as *const [Char16] as *const [u16]) }
    }
}
