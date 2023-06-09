use memory_structs::{FrameRange, Frame};
use crate::MemoryRegionType;
use core::{borrow::Borrow, cmp::{Ordering, min, max}, fmt, ops::{Deref, DerefMut}};

/// A region of contiguous frames.
/// Only used for bookkeeping, not for allocation.
///
/// # Ordering and Equality
///
/// `Region` implements the `Ord` trait, and its total ordering is ONLY based on
/// its **starting** `Frame`. This is useful so we can store `Region`s in a sorted collection.
///
/// Similarly, `Region` implements equality traits, `Eq` and `PartialEq`,
/// both of which are also based ONLY on the **starting** `Frame` of the `Region`.
/// Thus, comparing two `Region`s with the `==` or `!=` operators may not work as expected.
/// since it ignores their actual range of frames.
#[derive(Debug, Clone, Eq)]
pub struct Region {
    /// The type of this memory region, e.g., whether it's in a free or reserved region.
    pub(crate) typ: MemoryRegionType,
    /// The Frames covered by this region, an inclusive range. 
    pub(crate) frames: FrameRange,
}
impl Region {
    /// Returns a new `Region` with an empty range of frames. 
    pub fn empty() -> Region {
        Region {
            typ: MemoryRegionType::Unknown,
            frames: FrameRange::empty(),
        }
    }
}

impl Deref for Region {
    type Target = FrameRange;
    fn deref(&self) -> &FrameRange {
        &self.frames
    }
}
impl Ord for Region {
    fn cmp(&self, other: &Self) -> Ordering {
        self.frames.start().cmp(other.frames.start())
    }
}
impl PartialOrd for Region {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Region {
    fn eq(&self, other: &Self) -> bool {
        self.frames.start() == other.frames.start()
    }
}
impl Borrow<Frame> for &'_ Region {
    fn borrow(&self) -> &Frame {
        self.frames.start()
    }
}