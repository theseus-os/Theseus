//! An immutable shared reference to a string, effectively `Arc<str>`.

#![no_std]

extern crate alloc;

use core::{
    borrow::Borrow,
    hash::{Hash, Hasher},
    fmt,
    ops::Deref,
    str,
};
use alloc::sync::Arc;

/// A wrapper around an `Arc<str>`: an immutable shared reference to a string slice.
/// 
/// This can be borrowed and hashed as a slice of bytes because it implements `Borrow<[u8]>`, 
/// which is useful for compatibility with crates like `qp_trie`.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct StrRef(Arc<str>);

impl Deref for StrRef {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Clone for StrRef {
    fn clone(&self) -> Self {
        StrRef(Arc::clone(&self.0))
    }
}

impl fmt::Debug for StrRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl fmt::Display for StrRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for StrRef {
    fn from(s: &str) -> Self {
        StrRef(Arc::from(s))
    }
}

impl Borrow<str> for StrRef {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<[u8]> for StrRef {
    #[inline]
    fn borrow(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

#[allow(clippy::derive_hash_xor_eq)]
impl Hash for StrRef {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_bytes().hash(state);
    }
}

impl AsRef<str> for StrRef {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl StrRef {
    /// Obtain a reference to the inner `str`.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
