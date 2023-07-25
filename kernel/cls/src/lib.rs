#![no_std]

extern crate alloc;

pub use cls_macros::cpu_local;

// TODO: Support multiple integer sizes?
/// Trait for types that can be stored in a single register.
pub trait Raw {
    /// Returns a `u64` representation of the struct.
    fn into_raw(self) -> u64;

    /// Recreates the struct from the `u64` representation.
    ///
    /// # Safety
    ///
    /// The raw representation must have been previously returned from a call to
    /// [`<Self as RawRepresentation>::into_raw`]. Furthermore, `from_raw` must
    /// only be called once per `u64` returned from [`<Self as
    /// RawRepresentation>::into_raw`].
    ///
    /// [`<Self as RawRepresentation>::into_raw`]: RawRepresentation::into_raw
    unsafe fn from_raw(raw: u64) -> Self;
}

impl Raw for u8 {
    fn into_raw(self) -> u64 {
        u64::from(self)
    }

    unsafe fn from_raw(raw: u64) -> Self {
        // Guaranteed to fit since it was created by `into_raw`.
        Self::try_from(raw).unwrap()
    }
}

impl Raw for u16 {
    fn into_raw(self) -> u64 {
        u64::from(self)
    }

    unsafe fn from_raw(raw: u64) -> Self {
        // Guaranteed to fit since it was created by `into_raw`.
        Self::try_from(raw).unwrap()
    }
}

impl Raw for u32 {
    fn into_raw(self) -> u64 {
        u64::from(self)
    }

    unsafe fn from_raw(raw: u64) -> Self {
        // Guaranteed to fit since it was created by `into_raw`.
        Self::try_from(raw).unwrap()
    }
}

impl Raw for u64 {
    fn into_raw(self) -> u64 {
        self
    }

    unsafe fn from_raw(raw: u64) -> Self {
        raw
    }
}
