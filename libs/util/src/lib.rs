#![no_std]

/// the log base 2 of an integer value
pub fn log2(value: usize) -> usize {
    let mut v = value;
    let mut result = 0;
    v >>= 1;
    while v > 0 {
        result += 1;
        v >>= 1;
    }

    result
}


/// rounds the given `value` to the nearest base,
/// which must be a power of two
pub fn round_up_power_of_two(value: usize, base: usize) -> usize {
    (value + (base - 1)) & !(base - 1)
}


/// Rounds the given `value` up to the nearest `multiple`.
#[inline(always)]
pub fn round_up(value: usize, multiple: usize) -> usize {
    ((value + multiple - 1) / multiple) * multiple
}


/// Rounds the given `value` down to the nearest `multiple`.
#[inline(always)]
pub fn round_down(value: usize, multiple: usize) -> usize {
    (value / multiple) * multiple
}
