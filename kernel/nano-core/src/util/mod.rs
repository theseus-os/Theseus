#[macro_use] pub mod int_like;


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