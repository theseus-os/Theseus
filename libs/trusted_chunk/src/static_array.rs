use prusti_contracts::*;

#[cfg(prusti)]
use crate::external_spec::trusted_range_inclusive::*;
#[cfg(not(prusti))]
use range_inclusive::*;

use crate::{
    external_spec::{trusted_option::*, trusted_result::*},
    spec::range_overlaps::*
};
use core::mem;

pub struct StaticArray  {
    pub(crate) arr: [Option<RangeInclusive<usize>>; 64] // only public so it can be used in spec
}

impl StaticArray {
    pub const fn new() -> Self {
        StaticArray {
            arr: [None; 64]
        }
    }

    #[pure]
    #[ensures(result == self.arr.len())]
    pub const fn len(&self) -> usize {
        self.arr.len()
    }

    /// Looks up an element in the array.
    /// 
    /// # Pre-conditions:
    /// * index is less than the length
    #[pure]
    #[requires(0 <= index && index < self.len())]
    #[ensures(result == self.arr[index])]
    pub fn lookup(&self, index: usize) -> Option<RangeInclusive<usize>> {
        self.arr[index]
    }

    // /// Sets the the element at `index` with the value `elem`.
    // /// 
    // /// # Pre-conditions:
    // /// * Index is less than the length
    // /// 
    // /// # Post-conditions:
    // /// * The element at `index` is equal to `elem`
    // /// * All other elements not at `index` remain unchanged
    // #[requires(0 <= index && index < self.len())]
    // #[ensures(self.arr[index] == elem)]
    // #[ensures(forall(|i: usize| ((0 <= i && i < self.arr.len()) && i != index) ==> self.arr[i] == old(self.arr[i])))]
    // pub(crate) fn set_element(&mut self, index: usize, elem: Option<RangeInclusive<usize>>) {
    //     self.arr[index] = elem;
    // }


    // /// Looks up an element in the array and returns its start and end value.
    // /// This has to be trusted because we access the RangeInclusive start and end values
    // /// which we've already seen leads to internal compiler errors due to generics.
    // /// 
    // /// # Pre-conditions:
    // /// * Index is less than the length
    // /// 
    // /// # Post-conditions:
    // /// * The returned values are the start and end of the RangeInclusive at `index`
    // /// * If there is no element at `index`, then None is returned
    // #[pure]
    // #[trusted]
    // #[requires(0 <= index && index < self.len())]
    // #[ensures(result.is_some() ==> 
    //     self.arr[index].is_some() && 
    //     *peek_option(&self.arr[index]).start() == peek_option(&result).0 && 
    //     *peek_option(&self.arr[index]).end() == peek_option(&result).1
    // )]
    // #[ensures(result.is_none() ==> self.arr[index].is_none())]
    // pub fn lookup_range_bounds(&self, index: usize) -> Option<((usize, usize))> {
    //     match self.arr[index] {
    //         Some(val) => Some((*val.start(), *val.end())),
    //         None => None
    //     }
    // }


    /// Adds an element to the array if there's space.
    /// 
    /// # Pre-conditions:
    /// * The array is ordered so that all Some(_) elements occur at the beginning of the array, followed by all None elements.
    ///
    /// # Post-conditions:
    /// * If the push fails, then all elements remain unchanged
    /// * If the push fails, then all elements were Some(_)
    /// * If the push succeeds, then the element at the returned index is now Some(_)
    /// * If the push succeeds, then the element at the returned index is equal to `value`
    /// * If the push succeeds, then all the elements are unchanged except at the returned index 
    /// * If successful, then the array remains ordered with all Some elements followed by all None elements
    #[requires(forall(|i: usize| (0 <= i && i < self.arr.len() && self.arr[i].is_some()) ==> {
        forall(|j: usize| (0 <= j && j < i) ==> self.arr[j].is_some())
    }))]
    #[requires(forall(|i: usize| (0 <= i && i < self.arr.len() && self.arr[i].is_none()) ==> {
        forall(|j: usize| (i <= j && j < self.arr.len()) ==> self.arr[j].is_none())
    }))]
    #[ensures(result.is_err() ==> 
        forall(|i: usize| (0 <= i && i < self.arr.len()) ==> old(self.arr[i]) == self.arr[i])
    )]
    #[ensures(result.is_err() ==> 
        forall(|i: usize| (0 <= i && i < self.arr.len()) ==> self.arr[i].is_some())
    )]
    #[ensures(result.is_ok() ==> {
        let idx = peek_result(&result);
        self.arr[idx].is_some()
    })]
    #[ensures(result.is_ok() ==> {
        let idx = peek_result(&result);
        let val = peek_option(&self.arr[idx]);
        val == value
    })]
    #[ensures(result.is_ok() ==> 
        forall(|i: usize| ((0 <= i && i < self.arr.len()) && (i != peek_result(&result))) ==> old(self.arr[i]) == self.arr[i])
    )] 
    #[ensures(forall(|i: usize| (0 <= i && i < self.arr.len() && self.arr[i].is_some()) ==> {
        forall(|j: usize| (0 <= j && j < i) ==> self.arr[j].is_some())
    }))]
    #[ensures(forall(|i: usize| (0 <= i && i < self.arr.len() && self.arr[i].is_none()) ==> {
        forall(|j: usize| (i <= j && j < self.arr.len()) ==> self.arr[j].is_none())
    }))]
	pub(crate) fn push(&mut self, value: RangeInclusive<usize>) -> Result<usize,()> {
        let mut i = 0;

        while i < self.arr.len() {
            body_invariant!(forall(|j: usize| ((0 <= j && j < i) ==> self.arr[j].is_some())));
            body_invariant!(i < self.arr.len());
            body_invariant!(i >= 0);

            if self.arr[i].is_none() {
                self.arr[i] = Some(value);
                return Ok(i)
            }
            i += 1;
        }
        return Err(());
	}


    /// Returns the index of the first element in the array, starting from `index`, which overlaps with `elem`.
    /// Returns None if there is no overlap.
    ///  
    /// # Pre-conditions:
    /// * index is less than or equal to the array length
    /// 
    /// # Post-conditions:
    /// * if the result is Some(idx), then idx is less than the list's length.
    /// * if the result is Some(idx), then the element at idx is Some(_)
    /// * if the result is Some(idx), then the element at idx overlaps with `elem`
    /// * if the result is None, then no element in the array overlaps with `elem`
    #[requires(0 <= index && index <= self.arr.len())]
    #[ensures(result.is_some() ==> peek_option(&result) < self.arr.len())]
    #[ensures(result.is_some() ==> self.arr[peek_option(&result)].is_some())]
    #[ensures(result.is_some() ==> {
            let idx = peek_option(&result);
            let range = peek_option(&self.arr[idx]);
            range_overlaps(&range, &elem)
        }
    )]
    #[ensures(result.is_none() ==> 
        forall(|i: usize| ((index <= i && i < self.arr.len()) && self.arr[i].is_some()) ==> {
            let range = peek_option(&self.arr[i]);
            !range_overlaps(&range, &elem)
        })
    )]
    pub(crate) fn elem_overlaps_in_array(&self, elem: RangeInclusive<usize>, index: usize) -> Option<usize> {
        if index == self.arr.len() {
            return None;
        }

        let ret = match self.arr[index] {
            Some(val) => {
                if range_overlaps(&val,&elem) {
                    Some(index)
                } else {
                    self.elem_overlaps_in_array(elem, index + 1)
                }
            },
            None => {
                self.elem_overlaps_in_array(elem, index + 1)
            }
        };
        ret
    }
}