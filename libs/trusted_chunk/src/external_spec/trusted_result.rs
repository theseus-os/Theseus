//! Helper functions for `Result` to peek the inner value.
//! We don't have to write an extern spec for `Result` because it's already in prusti_contracts.

use prusti_contracts::*;

#[pure]
#[requires(val.is_ok())]
pub(crate) fn peek_result<T: Copy, E>(val: &Result<T,E>) -> T {
    match val {
        Ok(val) => *val,
        Err(_) => unreachable!(),
    }
}

#[pure]
#[requires(val.is_ok())]
pub(crate) fn peek_result_ref<T, E>(val: &Result<T,E>) -> &T {
    match val {
        Ok(val) => val,
        Err(_) => unreachable!(),
    }
}

#[pure]
#[requires(val.is_err())]
pub(crate) fn peek_err<T, E: Copy>(val: &Result<T,E>) -> E {
    match val {
        Ok(_) => unreachable!(),
        Err(e) => *e,
    }
}

#[pure]
#[requires(val.is_err())]
pub(crate) fn peek_err_ref<T, E>(val: &Result<T,E>) -> &E {
    match val {
        Ok(_) => unreachable!(),
        Err(e) => e,
    }
}
