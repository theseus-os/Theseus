//! This function defines what it means for two RangeInclusives to overlap. 
//! It is used in both the implementation and specification.
//! If this definition of overlapping doesn't match what the rest of the system expects, then the results of verification won't make sense.

use prusti_contracts::*;

#[cfg(prusti)]
use crate::external_spec::trusted_range_inclusive::*;
#[cfg(not(prusti))]
use range_inclusive::*;

use core::cmp::{max,min};


/// Returns false if either of `range1` or `range2` are empty, or if there is no overlap.
/// Returns true otherwise.
/// 
/// # Verification Notes:
/// * Removing the trusted marking leads to "unexpected internal errors", I think because of issues with generics in RangeInclusive.
/// * To make sure the post-conditions are valid, I implemented a range_overlaps_check function (EOF) which is identical except
/// it works with a Range type without generics, and it verifies.
#[pure]
#[trusted] // Only trusted functions can call themselves in their contracts
#[ensures(!result ==> (
    *range1.start() > *range1.end() ||
    *range2.start() > *range2.end() ||
    (*range1.start() <= *range1.end() && *range2.start() <= *range2.end() && (*range2.start() > *range1.end() || *range1.start() > *range2.end()))
))]
#[ensures(result ==> (*range2.start() <= *range1.end() || *range1.start() <= *range2.end()))]
#[ensures(result ==> range_overlaps(range2, range1))]
pub fn range_overlaps(range1: &RangeInclusive<usize>, range2: &RangeInclusive<usize>) -> bool {
    if range1.is_empty() || range2.is_empty() {
        return false;
    }

    let starts = max(range1.start(), range2.start());
    let ends = min(range1.end(), range2.end());
    starts <= ends
}



#[cfg(test)]
mod test {
    use range_inclusive::RangeInclusive;

    use super::range_overlaps;

    #[test]
    fn overlapping() {
        let r1 = RangeInclusive::new(0,5);
        let r2 = RangeInclusive::new(4, 7);
        assert!(range_overlaps(&r1, &r2));

        let r1 = RangeInclusive::new(0,5);
        let r2 = RangeInclusive::new(5, 5);
        assert!(range_overlaps(&r1, &r2));
    }

    #[test]
    fn not_overlapping() {
        // empty ranges
        let r1 = RangeInclusive::new(5,0);
        let r2 = RangeInclusive::new(4, 7);
        assert!(!range_overlaps(&r1, &r2));

        // empty ranges
        let r1 = RangeInclusive::new(5,5);
        let r2 = RangeInclusive::new(6, 5);
        assert!(!range_overlaps(&r1, &r2));

        let r1 = RangeInclusive::new(3,5);
        let r2 = RangeInclusive::new(6, 7);
        assert!(!range_overlaps(&r1, &r2));
    }

}



// #[pure]
// #[trusted]
// #[ensures(result ==> range_overlaps(range2, range1))]
// pub fn range_overlaps<Idx: Clone + PartialOrd + Ord>(range1: &RangeInclusive<Idx>, range2: &RangeInclusive<Idx>) -> bool {
//     if range1.is_empty() || range2.is_empty() {
//         return false;
//     }

//     let starts = if range1.start() >= range2.start() {
//         range1.start()
//     } else {
//         range2.start()
//     };

//     let ends = if range1.end() <= range2.end() {
//         range1.end()
//     } else {
//         range2.end()
//     };

//     starts <= ends
// }



// *** Code below is a sanity check to make sure our trusted function for range_overlaps is correct ***//
// struct Range {
//     start: usize,
//     end: usize
// }

// impl Range {
//     #[pure]
//     fn start(&self) -> usize {
//         self.start
//     }
//     #[pure]
//     fn end(&self) -> usize {
//         self.end
//     }
//     #[pure]
//     fn is_empty(&self) -> bool {
//         !(self.start <= self.end)
//     }
// }

// #[pure]
// #[ensures(!result ==> (
//     range1.start() > range1.end() ||
//     range2.start() > range2.end() ||
//     (range1.start() <= range1.end() && range2.start() <= range2.end() && (range2.start() > range1.end() || range1.start() > range2.end()))
// ))]
// #[ensures(result ==> (range2.start() <= range1.end() || range1.start() <= range2.end()))]
// fn range_overlaps_check(range1: &Range, range2: &Range) -> bool {
//     if range1.is_empty() || range2.is_empty() {
//         return false;
//     }

//     let starts = max_usize(range1.start(), range2.start());
//     let ends = min_usize(range1.end(), range2.end());
//     starts <= ends
// }

// #[pure]
// fn max_usize(a: usize, b: usize) -> usize {
//     if a >= b { a } else { b }
// }

// #[pure]
// fn min_usize(a: usize, b: usize) -> usize {
//     if a <= b { a } else { b }
// }