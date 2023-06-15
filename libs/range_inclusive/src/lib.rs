//! A `RangeInclusive` implementation which creates an iterator that doesn't mutate the original range.
//! This is accomplished by requiring that the generic type `Idx` implements the `Clone` trait.
//! 
//! All functions except for the iterators conform to the specification given for std::ops::RangeInclusive.

#![no_std]
#![feature(step_trait)]

#[cfg(test)]
mod test;

use core::iter::Step;
use core::ops::{RangeBounds, Bound, Bound::Included};

/// A range bounded inclusively below and above (`start..=end`).
///
/// The `RangeInclusive` `start..=end` contains all values with `x >= start`
/// and `x <= end`. It is empty unless `start <= end`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct RangeInclusive<Idx: Clone + PartialOrd> {
    pub(crate) start: Idx,
    pub(crate) end: Idx
}

impl<Idx: Clone + PartialOrd> RangeInclusive<Idx> {
    /// Creates a new inclusive range.
    #[inline]
    pub const fn new(start: Idx, end: Idx) -> Self {
        Self{ start, end }
    }

    /// Returns the lower bound of the range (inclusive).
    #[inline]
    pub const fn start(&self) -> &Idx {
        &self.start
    }

    /// Returns the upper bound of the range (inclusive).
    #[inline]
    pub const fn end(&self) -> &Idx {
        &self.end
    }

    /// Destructures the `RangeInclusive` into (lower bound, upper (inclusive) bound).
    #[inline]
    pub fn into_inner(self) -> (Idx, Idx) {
        (self.start, self.end)
    }

    /// Returns `true` if the range contains no items.
    pub fn is_empty(&self) -> bool {
        !(self.start <= self.end)
    }

    /// Returns an iterator with the same `start` and `end` values as the range.
    pub fn iter(&self) -> RangeInclusiveIterator<Idx> {
        RangeInclusiveIterator { current: self.start.clone(), end: self.end.clone() }
    }

    /// Returns `true` if `item` is contained in the range.
    pub fn contains<U>(&self, item: &U) -> bool
    where
        Idx: PartialOrd<U>,
        U: ?Sized + PartialOrd<Idx>,
    {
        <Self as RangeBounds<Idx>>::contains(self, item)
    }
}

impl<T: Clone + PartialOrd> RangeBounds<T> for RangeInclusive<T> {
    fn start_bound(&self) -> Bound<&T> {
        Included(&self.start)
    }
    fn end_bound(&self) -> Bound<&T> {
        Included(&self.end)
    }
}

impl<'a, Idx: Clone + PartialOrd + Step> IntoIterator for &'a RangeInclusive<Idx> {
    type Item = Idx;
    type IntoIter = RangeInclusiveIterator<Idx>;

    fn into_iter(self) -> RangeInclusiveIterator<Idx> {
        self.iter()
    }
}

/// An iterator for the `RangeInclusive` type.
/// By creating a separate iterator, we don't need to mutate the original range.
pub struct RangeInclusiveIterator<Idx> {
    /// The current value of the iterator which is returned by `next()`.
    current: Idx,
    /// The end value of the iterator.
    end: Idx
}

impl<A: Step> Iterator for RangeInclusiveIterator<A> {
    type Item = A;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current > self.end {
            None
        } else {
            let n = Step::forward_checked(self.current.clone(), 1).expect("`Step` invariants not upheld");
            Some(core::mem::replace(&mut self.current, n))
        }
    }
}

impl<A: Step> ExactSizeIterator for RangeInclusiveIterator<A> {
    fn len(&self) -> usize {
        Step::steps_between(&self.current, &self.end).map(|x| x+1).unwrap_or(0)
    }
}

impl<A: Step> DoubleEndedIterator for RangeInclusiveIterator<A>  {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.current > self.end {
            None
        } else {
            let n = Step::backward_checked(self.end.clone(), 1).expect("`Step` invariants not upheld");
            Some(core::mem::replace(&mut self.end, n))
        }
    }
}
