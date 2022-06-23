use super::chunk_range_wrapper::ChunkRangeWrapper;
use crate::{allocator::StaticArrayRBTree, Address, Chunk, Virtual};
use kernel_config::memory::KERNEL_HEAP_START;
use spin::{Mutex, Once};

pub(crate) static DESIGNATED_PAGES_LOW_END: Once<Chunk<Virtual>> = Once::new();

pub(crate) static DESIGNATED_PAGES_HIGH_START: Chunk<Virtual> =
    Chunk::containing_address(Address::new_canonical(KERNEL_HEAP_START));

pub(crate) static FREE_PAGE_LIST: Mutex<StaticArrayRBTree<ChunkRangeWrapper<Virtual>>> =
    Mutex::new(StaticArrayRBTree::empty());
