//! The specification for the `Option` type and helper functions to peek the inner value.

use prusti_contracts::*;
use crate::external_spec::trusted_range_inclusive::*;

#[extern_spec]
impl<T: PartialEq + Copy> core::option::Option<T> {
    #[pure]
    #[ensures(matches!(*self, Some(_)) == result)]
    pub fn is_some(&self) -> bool;

    #[pure]
    #[ensures(self.is_some() == !result)]
    pub fn is_none(&self) -> bool;

    #[requires(self.is_some())]
    #[ensures(result == peek_option(&self))]
    pub fn unwrap(self) -> T;

    #[ensures(result.is_some() ==> (old(self.is_some()) && old(peek_option(&self)) == peek_option(&result)))]
    #[ensures(result.is_none() ==> old(self.is_none()))]
    pub fn take(&mut self) -> Option<T>;
}

#[extern_spec]
impl<T: Copy + PartialEq> PartialEq for core::option::Option<T> {
    #[pure]
    #[ensures(self.is_none() && other.is_none() ==> result == true)]
    #[ensures(self.is_none() && other.is_some() ==> result == false)]
    #[ensures(self.is_some() && other.is_none() ==> result == false)]
    #[ensures(self.is_some() && other.is_some() ==> {
        let val1 = peek_option(&self);
        let val2 = peek_option(&other);
        result == (val1 == val2)
    })]
    fn eq(&self, other:&Self) -> bool;
}

#[pure]
#[requires(val.is_some())]
pub(crate) fn peek_option<T: Copy>(val: &Option<T>) -> T {
    match val {
        Some(val) => *val,
        None => unreachable!(),
    }
}

#[pure]
#[requires(val.is_some())]
pub(crate) fn peek_option_ref<T>(val: &Option<T>) -> &T {
    match val {
        Some(val) => val,
        None => unreachable!(),
    }
}
