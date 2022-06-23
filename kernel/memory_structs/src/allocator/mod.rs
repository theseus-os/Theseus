// TODO: Remove
#![allow(dead_code)]

use crate::MemoryType;
use core::marker::PhantomData;

pub(crate) mod allocated;
pub(crate) mod chunk_range_wrapper;
mod physical;
mod static_array_rb_tree;
mod virt;

pub use allocated::AllocatedChunks;
pub(crate) use static_array_rb_tree::StaticArrayRBTree;

pub struct Allocator<T>
where
    T: MemoryType,
{
    phantom_data: PhantomData<fn() -> T>,
}
