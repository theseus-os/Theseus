//! The underlying `OsString`/`OsStr` implementation for Theseus.
//! 
//! This is very simple: Theseus uses Rust strings as its native string type. 

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::collections::TryReserveError;
use alloc::string::ToString;
use core::fmt;
use core::mem;
use alloc::rc::Rc;
use core::str;
use alloc::sync::Arc;
use alloc::string::String;
use crate::sys_common::{AsInner, IntoInner};


#[derive(Clone, Debug, Hash)]
#[repr(transparent)]
pub struct Buf {
    pub inner: String,
}

impl fmt::Display for Buf {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, formatter)
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct Slice {
    pub inner: str,
}

impl fmt::Display for Slice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, formatter)
    }
}

impl IntoInner<String> for Buf {
    fn into_inner(self) -> String {
        self.inner
    }
}

impl AsInner<str> for Buf {
    fn as_inner(&self) -> &str {
        &self.inner
    }
}

impl Buf {
    pub fn from_string(s: String) -> Buf {
        Buf { inner: s }
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Buf {
        Buf { inner: String::with_capacity(capacity) }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear()
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional)
    }

    #[inline]
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.inner.try_reserve(additional)
    }

    #[inline]
    pub fn reserve_exact(&mut self, additional: usize) {
        self.inner.reserve_exact(additional)
    }

    #[inline]
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.inner.try_reserve_exact(additional)
    }

    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.inner.shrink_to_fit()
    }

    #[inline]
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.inner.shrink_to(min_capacity)
    }

    #[inline]
    pub fn as_slice(&self) -> &Slice {
        // SAFETY: Slice just wraps str,
        // and &*self.inner is &str, therefore
        // transmuting &str to &Slice is safe.
        unsafe { mem::transmute(&*self.inner) }
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut Slice {
        // SAFETY: Slice just wraps str,
        // and &mut *self.inner is &mut str, therefore
        // transmuting &mut str to &mut Slice is safe.
        unsafe { mem::transmute(&mut *self.inner) }
    }

    /// This always succeeds on Theseus.
    pub fn into_string(self) -> Result<String, Buf> {
        Ok(self.inner)
    }

    pub fn push_slice(&mut self, s: &Slice) {
        self.inner.push_str(&s.inner)
    }

    #[inline]
    pub fn into_box(self) -> Box<Slice> {
        unsafe { mem::transmute(self.inner.into_boxed_str()) }
    }

    #[inline]
    pub fn from_box(boxed: Box<Slice>) -> Buf {
        let inner: Box<str> = unsafe { mem::transmute(boxed) };
        Buf { inner: inner.into_string() }
    }

    #[inline]
    pub fn into_arc(&self) -> Arc<Slice> {
        self.as_slice().into_arc()
    }

    #[inline]
    pub fn into_rc(&self) -> Rc<Slice> {
        self.as_slice().into_rc()
    }
}

impl Slice {
    #[inline]
    pub fn from_str(s: &str) -> &Slice {
        unsafe { mem::transmute(s) }
    }

    /// Always returns `Some` in Theseus.
    pub fn to_str(&self) -> Option<&str> {
        Some(&self.inner)
    }

    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.inner)
    }

    pub fn to_owned(&self) -> Buf {
        Buf { inner: self.inner.to_string() }
    }

    pub fn clone_into(&self, buf: &mut Buf) {
        buf.inner = String::from(&self.inner);
    }

    #[inline]
    pub fn into_box(&self) -> Box<Slice> {
        let boxed: Box<str> = self.inner.into();
        unsafe { mem::transmute(boxed) }
    }

    pub fn empty_box() -> Box<Slice> {
        let boxed: Box<str> = Default::default();
        unsafe { mem::transmute(boxed) }
    }

    #[inline]
    pub fn into_arc(&self) -> Arc<Slice> {
        let arc: Arc<str> = Arc::from(&self.inner);
        unsafe { Arc::from_raw(Arc::into_raw(arc) as *const Slice) }
    }

    #[inline]
    pub fn into_rc(&self) -> Rc<Slice> {
        let rc: Rc<str> = Rc::from(&self.inner);
        unsafe { Rc::from_raw(Rc::into_raw(rc) as *const Slice) }
    }

    #[inline]
    pub fn make_ascii_lowercase(&mut self) {
        self.inner.make_ascii_lowercase()
    }

    #[inline]
    pub fn make_ascii_uppercase(&mut self) {
        self.inner.make_ascii_uppercase()
    }

    #[inline]
    pub fn to_ascii_lowercase(&self) -> Buf {
        Buf { inner: self.inner.to_ascii_lowercase() }
    }

    #[inline]
    pub fn to_ascii_uppercase(&self) -> Buf {
        Buf { inner: self.inner.to_ascii_uppercase() }
    }

    #[inline]
    pub fn is_ascii(&self) -> bool {
        self.inner.is_ascii()
    }

    #[inline]
    pub fn eq_ignore_ascii_case(&self, other: &Self) -> bool {
        self.inner.eq_ignore_ascii_case(&other.inner)
    }
}
