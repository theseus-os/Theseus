//! A specification for `RangeInclusive` which is just an actual type definition and relevant functions.
//! We don't use a Prusti external spec here because of dependency errors when using prusti-rustc (maybe due to a git path?) 
//! and Prusti errors about the return types of pure functions and some "unexpected panics" when using cargo-prusti.

use prusti_contracts::*;

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct RangeInclusive<Idx: Clone + PartialOrd> {
    start: Idx,
    end: Idx
}

impl<Idx: Clone + PartialOrd> RangeInclusive<Idx> {
    #[ensures(result.start == start)]
    #[ensures(result.end == end)]
    pub(crate) const fn new(start: Idx, end: Idx) -> Self {
        Self{start, end}
    }

    #[pure]
    pub const fn start(&self) -> &Idx {
        &self.start
    }

    #[pure]
    pub const fn end(&self) -> &Idx {
        &self.end
    }

    pub fn is_empty(&self) -> bool {
        !(self.start <= self.end)
    }

}
