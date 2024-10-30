#![cfg_attr(not(std), no_std)]
#![feature(box_patterns)]
#![allow(unused)]
#![feature(step_trait)]
#![feature(rustc_private)]

#[macro_use]
extern crate prusti_contracts;
#[macro_use]
extern crate cfg_if;
extern crate core;

pub(crate) mod spec;
pub(crate) mod external_spec;
pub mod linked_list;
pub mod static_array;
pub mod trusted_chunk;
mod test;

cfg_if::cfg_if! {
if #[cfg(prusti)] {
    use crate::external_spec::trusted_range_inclusive::*;
} else {
    extern crate alloc;
    extern crate range_inclusive;
    extern crate spin;
    use range_inclusive::*;

    static INIT: spin::Once<bool> = spin::Once::new();
}
}


#[cfg(not(prusti))] // prusti can't reason about fn pointers
pub fn init() -> Result<fn(RangeInclusive<usize>) -> trusted_chunk::TrustedChunk, &'static str> {
    if INIT.is_completed() {
        Err("Trusted Chunk has already been initialized and callback has been returned")
    } else {
        INIT.call_once(|| true);
        Ok(create_from_unmapped)
    }
}

#[requires(*frames.start() <= *frames.end())]
#[ensures(result.start() == *frames.start())]
#[ensures(result.end() == *frames.end())]
fn create_from_unmapped(frames: RangeInclusive<usize>) -> trusted_chunk::TrustedChunk {
    trusted_chunk::TrustedChunk::trusted_new(frames)
}
