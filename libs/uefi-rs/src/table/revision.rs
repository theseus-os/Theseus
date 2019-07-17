use core::fmt;

/// A revision of the UEFI specification.
///
/// The major revision number is incremented on major, API-incompatible changes.
///
/// The minor revision number is incremented on minor changes,
/// it is stored as a two-digit binary-coded decimal.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Revision(u32);

impl Revision {
    /// Creates a new revision.
    pub fn new(major: u16, minor: u16) -> Self {
        let (major, minor) = (u32::from(major), u32::from(minor));
        let value = (major << 16) | minor;
        Revision(value)
    }

    /// Returns the major revision.
    pub fn major(self) -> u16 {
        (self.0 >> 16) as u16
    }

    /// Returns the minor revision.
    pub fn minor(self) -> u16 {
        self.0 as u16
    }
}

impl fmt::Debug for Revision {
    /// Formats the revision in the `major.minor.patch` format.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (major, minor) = (self.major(), self.minor());
        write!(f, "{}.{}.{}", major, minor / 10, minor % 10)
    }
}
