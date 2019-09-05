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