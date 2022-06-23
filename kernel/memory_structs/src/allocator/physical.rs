use super::{
    chunk_range_wrapper::ChunkRangeWrapper, static_array_rb_tree::Inner, StaticArrayRBTree,
};
use crate::{Chunk, Physical};
use core::ops::Deref;
use intrusive_collections::Bound;
use spin::Mutex;

/// The single, system-wide list of free physical memory frames available for general usage.
pub(crate) static FREE_GENERAL_FRAMES_LIST: Mutex<StaticArrayRBTree<ChunkRangeWrapper<Physical>>> =
    Mutex::new(StaticArrayRBTree::empty());
/// The single, system-wide list of free physical memory frames reserved for specific usage.
pub(crate) static FREE_RESERVED_FRAMES_LIST: Mutex<StaticArrayRBTree<ChunkRangeWrapper<Physical>>> =
    Mutex::new(StaticArrayRBTree::empty());

/// The fixed list of all known regions that are available for general use.
/// This does not indicate whether these regions are currently allocated,
/// rather just where they exist and which regions are known to this allocator.
pub(crate) static GENERAL_REGIONS: Mutex<StaticArrayRBTree<ChunkRangeWrapper<Physical>>> =
    Mutex::new(StaticArrayRBTree::empty());
/// The fixed list of all known regions that are reserved for specific purposes.
/// This does not indicate whether these regions are currently allocated,
/// rather just where they exist and which regions are known to this allocator.
pub(crate) static RESERVED_REGIONS: Mutex<StaticArrayRBTree<ChunkRangeWrapper<Physical>>> =
    Mutex::new(StaticArrayRBTree::empty());

/// Returns whether the given `Chunk<Physical>` is contained within the given `list`.
pub(crate) fn frame_is_in_list(
    list: &StaticArrayRBTree<ChunkRangeWrapper<Physical>>,
    frame: &Chunk<Physical>,
) -> bool {
    match &list.0 {
        Inner::Array(ref arr) => {
            for chunk in arr.iter().flatten() {
                if chunk.contains(frame) {
                    return true;
                }
            }
        }
        Inner::RBTree(ref tree) => {
            let cursor = tree.upper_bound(Bound::Included(frame));
            if let Some(chunk) = cursor.get().map(|w| w.deref()) {
                if chunk.contains(frame) {
                    return true;
                }
            }
        }
    }

    false
}
