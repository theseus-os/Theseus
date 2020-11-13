//! This module is a hack to get around the lack of the 
//! `__truncdfsf2` function in the `compiler_builtins` crate.
//! See this: <https://github.com/rust-lang-nursery/compiler-builtins/pull/262>
//!
//! This code was taken from this commit from above-linked pull request to the compiler_builtins crate:
//! <https://github.com/rust-lang-nursery/compiler-builtins/commit/baab4fd89cdd945e46fed31166e5dcad7224ed87>
//! 
//! Unfortunately, it was later reverted since it caused failures on ARM, so I don't know how else to resolve this. 
//! 
//! ----------------------------------------------------------------------------------
//! 
//! In addition, PR #358 caused various compiler_builtins traits to be private, so we cannot use them any more. 
//! See here: <https://github.com/rust-lang/compiler-builtins/pull/358> 
//! 
//! Thus, we copy the contents and implementation of the Float, Int, and CastInto traits here
//! in the modules at the end of this file. This isn't a great solution, but it's easy, and temporary anyways. 
//! 

use self::float::Float;
use self::int::{CastInto, Int};

/// Generic conversion from a wider to a narrower IEEE-754 floating-point type
#[allow(dead_code)]
fn truncate<F: Float, R: Float>(a: F) -> R
where
    F::Int: CastInto<u64>,
    u64: CastInto<F::Int>,
    F::Int: CastInto<u32>,
    u32: CastInto<F::Int>,
    u32: CastInto<R::Int>,
    R::Int: CastInto<u32>,
    F::Int: CastInto<R::Int>,
{
    let src_one = F::Int::ONE;
    let src_bits = F::BITS;
    let src_sign_bits = F::SIGNIFICAND_BITS;
    let src_exp_bias = F::EXPONENT_BIAS;
    let src_min_normal = F::IMPLICIT_BIT;
    let src_infinity = F::EXPONENT_MASK;
    let src_sign_mask = F::SIGN_MASK as F::Int;
    let src_abs_mask = src_sign_mask - src_one;
    let src_qnan = F::SIGNIFICAND_MASK;
    let src_nan_code = src_qnan - src_one;

    let dst_bits = R::BITS;
    let dst_sign_bits = R::SIGNIFICAND_BITS;
    let dst_inf_exp = R::EXPONENT_MAX;
    let dst_exp_bias = R::EXPONENT_BIAS;

    let dst_zero = R::Int::ZERO;
    let dst_one = R::Int::ONE;
    let dst_qnan = R::SIGNIFICAND_MASK;
    let dst_nan_code = dst_qnan - dst_one;

    let round_mask = (src_one << src_sign_bits - dst_sign_bits) - src_one;
    let half = src_one << src_sign_bits - dst_sign_bits - 1;
    let underflow_exp = src_exp_bias + 1 - dst_exp_bias;
    let overflow_exp = src_exp_bias + dst_inf_exp - dst_exp_bias;
    let underflow: F::Int = underflow_exp.cast(); // << src_sign_bits;
    let overflow: F::Int = overflow_exp.cast(); //<< src_sign_bits;

    let a_abs = a.repr() & src_abs_mask;
    let sign = a.repr() & src_sign_mask;
    let mut abs_result: R::Int;

    let src_underflow = underflow << src_sign_bits;
    let src_overflow = overflow << src_sign_bits;

    if a_abs.wrapping_sub(src_underflow) < a_abs.wrapping_sub(src_overflow) {
        // The exponent of a is within the range of normal numbers
        let bias_delta: R::Int = (src_exp_bias - dst_exp_bias).cast();
        abs_result = a_abs.cast();
        abs_result = abs_result >> src_sign_bits - dst_sign_bits;
        abs_result = abs_result - bias_delta.wrapping_shl(dst_sign_bits);
        let round_bits: F::Int = a_abs & round_mask;
        abs_result += if round_bits > half {
            dst_one
        } else {
            abs_result & dst_one
        };
    } else if a_abs > src_infinity {
        // a is NaN.
        // Conjure the result by beginning with infinity, setting the qNaN
        // bit and inserting the (truncated) trailing NaN field
        let nan_result: R::Int = (a_abs & src_nan_code).cast();
        abs_result = dst_inf_exp.cast();
        abs_result = abs_result.wrapping_shl(dst_sign_bits);
        abs_result |= dst_qnan;
        abs_result |= (nan_result >> (src_sign_bits - dst_sign_bits)) & dst_nan_code;
    } else if a_abs >= src_overflow {
        // a overflows to infinity.
        abs_result = dst_inf_exp.cast();
        abs_result = abs_result.wrapping_shl(dst_sign_bits);
    } else {
        // a underflows on conversion to the destination type or is an exact
        // zero. The result may be a denormal or zero. Extract the exponent
        // to get the shift amount for the denormalization.
        let a_exp = a_abs >> src_sign_bits;
        let mut shift: u32 = a_exp.cast();
        shift = src_exp_bias - dst_exp_bias - shift + 1;

        let significand = (a.repr() & src_sign_mask) | src_min_normal;
        if shift > src_sign_bits {
            abs_result = dst_zero;
        } else {
            let sticky = significand << src_bits - shift;
            let mut denormalized_significand: R::Int = significand.cast();
            let sticky_shift: u32 = sticky.cast();
            denormalized_significand = denormalized_significand >> (shift | sticky_shift);
            abs_result = denormalized_significand >> src_sign_bits - dst_sign_bits;
            let round_bits = denormalized_significand & round_mask.cast();
            if round_bits > half.cast() {
                abs_result += dst_one; // Round to nearest
            } else if round_bits == half.cast() {
                abs_result += abs_result & dst_one; // Ties to even
            }
        }
    }
    // Finally apply the sign bit
    let s = sign >> src_bits - dst_bits;
    R::from_repr(abs_result | s.cast())
}

#[no_mangle]
pub extern "C" fn  __truncdfsf2(a: f64) -> f32 {
    truncate(a)
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn  __truncdfsf2vfp(a: f64) -> f32 {
    a as f32
}


/// This module is taken directly from `compiler_builtins/src/float/mod.rs`.
mod float {
    use core::mem;
    use core::ops;

    use super::int::Int;

    /// Trait for some basic operations on floats
    pub(crate) trait Float:
        Copy
        + PartialEq
        + PartialOrd
        + ops::AddAssign
        + ops::MulAssign
        + ops::Add<Output = Self>
        + ops::Sub<Output = Self>
        + ops::Div<Output = Self>
        + ops::Rem<Output = Self>
    {
        /// A uint of the same with as the float
        type Int: Int;

        /// A int of the same with as the float
        type SignedInt: Int;

        const ZERO: Self;
        const ONE: Self;

        /// The bitwidth of the float type
        const BITS: u32;

        /// The bitwidth of the significand
        const SIGNIFICAND_BITS: u32;

        /// The bitwidth of the exponent
        const EXPONENT_BITS: u32 = Self::BITS - Self::SIGNIFICAND_BITS - 1;

        /// The maximum value of the exponent
        const EXPONENT_MAX: u32 = (1 << Self::EXPONENT_BITS) - 1;

        /// The exponent bias value
        const EXPONENT_BIAS: u32 = Self::EXPONENT_MAX >> 1;

        /// A mask for the sign bit
        const SIGN_MASK: Self::Int;

        /// A mask for the significand
        const SIGNIFICAND_MASK: Self::Int;

        // The implicit bit of the float format
        const IMPLICIT_BIT: Self::Int;

        /// A mask for the exponent
        const EXPONENT_MASK: Self::Int;

        /// Returns `self` transmuted to `Self::Int`
        fn repr(self) -> Self::Int;

        /// Returns `self` transmuted to `Self::SignedInt`
        fn signed_repr(self) -> Self::SignedInt;

        #[cfg(test)]
        /// Checks if two floats have the same bit representation. *Except* for NaNs! NaN can be
        /// represented in multiple different ways. This method returns `true` if two NaNs are
        /// compared.
        fn eq_repr(self, rhs: Self) -> bool;

        /// Returns a `Self::Int` transmuted back to `Self`
        fn from_repr(a: Self::Int) -> Self;

        /// Constructs a `Self` from its parts. Inputs are treated as bits and shifted into position.
        fn from_parts(sign: bool, exponent: Self::Int, significand: Self::Int) -> Self;

        /// Returns (normalized exponent, normalized significand)
        fn normalize(significand: Self::Int) -> (i32, Self::Int);
    }

    // Some of this can be removed if RFC Issue #1424 is resolved
    // https://github.com/rust-lang/rfcs/issues/1424
    macro_rules! float_impl {
        ($ty:ident, $ity:ident, $sity:ident, $bits:expr, $significand_bits:expr) => {
            impl Float for $ty {
                type Int = $ity;
                type SignedInt = $sity;
                const ZERO: Self = 0.0;
                const ONE: Self = 1.0;

                const BITS: u32 = $bits;
                const SIGNIFICAND_BITS: u32 = $significand_bits;

                const SIGN_MASK: Self::Int = 1 << (Self::BITS - 1);
                const SIGNIFICAND_MASK: Self::Int = (1 << Self::SIGNIFICAND_BITS) - 1;
                const IMPLICIT_BIT: Self::Int = 1 << Self::SIGNIFICAND_BITS;
                const EXPONENT_MASK: Self::Int = !(Self::SIGN_MASK | Self::SIGNIFICAND_MASK);

                fn repr(self) -> Self::Int {
                    unsafe
                     { mem::transmute(self) }
                }
                fn signed_repr(self) -> Self::SignedInt {
                    unsafe { mem::transmute(self) }
                }
                #[cfg(test)]
                fn eq_repr(self, rhs: Self) -> bool {
                    if self.is_nan() && rhs.is_nan() {
                        true
                    } else {
                        self.repr() == rhs.repr()
                    }
                }
                fn from_repr(a: Self::Int) -> Self {
                    unsafe { mem::transmute(a) }
                }
                fn from_parts(sign: bool, exponent: Self::Int, significand: Self::Int) -> Self {
                    Self::from_repr(
                        ((sign as Self::Int) << (Self::BITS - 1))
                            | ((exponent << Self::SIGNIFICAND_BITS) & Self::EXPONENT_MASK)
                            | (significand & Self::SIGNIFICAND_MASK),
                    )
                }
                fn normalize(significand: Self::Int) -> (i32, Self::Int) {
                    let shift = significand
                        .leading_zeros()
                        .wrapping_sub((Self::Int::ONE << Self::SIGNIFICAND_BITS).leading_zeros());
                    (
                        1i32.wrapping_sub(shift as i32),
                        significand << shift as Self::Int,
                    )
                }
            }
        };
    }

    float_impl!(f32, u32, i32, 32, 23);
    float_impl!(f64, u64, i64, 64, 52);
}


/// This module is taken directly from `compiler_builtins/src/in/mod.rs`.
mod int {
    use core::ops;

    /// Trait for some basic operations on integers
    pub(crate) trait Int:
        Copy
        + PartialEq
        + PartialOrd
        + ops::AddAssign
        + ops::BitAndAssign
        + ops::BitOrAssign
        + ops::ShlAssign<i32>
        + ops::ShrAssign<u32>
        + ops::Add<Output = Self>
        + ops::Sub<Output = Self>
        + ops::Div<Output = Self>
        + ops::Shl<u32, Output = Self>
        + ops::Shr<u32, Output = Self>
        + ops::BitOr<Output = Self>
        + ops::BitXor<Output = Self>
        + ops::BitAnd<Output = Self>
        + ops::Not<Output = Self>
    {
        /// Type with the same width but other signedness
        type OtherSign: Int;
        /// Unsigned version of Self
        type UnsignedInt: Int;

        /// The bitwidth of the int type
        const BITS: u32;

        const ZERO: Self;
        const ONE: Self;

        /// Extracts the sign from self and returns a tuple.
        ///
        /// # Examples
        ///
        /// ```rust,ignore
        /// let i = -25_i32;
        /// let (sign, u) = i.extract_sign();
        /// assert_eq!(sign, true);
        /// assert_eq!(u, 25_u32);
        /// ```
        fn extract_sign(self) -> (bool, Self::UnsignedInt);

        fn unsigned(self) -> Self::UnsignedInt;
        fn from_unsigned(unsigned: Self::UnsignedInt) -> Self;

        fn from_bool(b: bool) -> Self;

        // copied from primitive integers, but put in a trait
        fn max_value() -> Self;
        fn min_value() -> Self;
        fn wrapping_add(self, other: Self) -> Self;
        fn wrapping_mul(self, other: Self) -> Self;
        fn wrapping_sub(self, other: Self) -> Self;
        fn wrapping_shl(self, other: u32) -> Self;
        fn overflowing_add(self, other: Self) -> (Self, bool);
        fn aborting_div(self, other: Self) -> Self;
        fn aborting_rem(self, other: Self) -> Self;
        fn leading_zeros(self) -> u32;
    }

    macro_rules! int_impl_common {
        ($ty:ty, $bits:expr) => {
            const BITS: u32 = $bits;

            const ZERO: Self = 0;
            const ONE: Self = 1;

            fn from_bool(b: bool) -> Self {
                b as $ty
            }

            fn max_value() -> Self {
                <Self>::max_value()
            }

            fn min_value() -> Self {
                <Self>::min_value()
            }

            fn wrapping_add(self, other: Self) -> Self {
                <Self>::wrapping_add(self, other)
            }

            fn wrapping_mul(self, other: Self) -> Self {
                <Self>::wrapping_mul(self, other)
            }

            fn wrapping_sub(self, other: Self) -> Self {
                <Self>::wrapping_sub(self, other)
            }

            fn wrapping_shl(self, other: u32) -> Self {
                <Self>::wrapping_shl(self, other)
            }

            fn overflowing_add(self, other: Self) -> (Self, bool) {
                <Self>::overflowing_add(self, other)
            }

            fn aborting_div(self, other: Self) -> Self {
                <Self>::checked_div(self, other).unwrap()
            }

            fn aborting_rem(self, other: Self) -> Self {
                <Self>::checked_rem(self, other).unwrap()
            }

            fn leading_zeros(self) -> u32 {
                <Self>::leading_zeros(self)
            }
        };
    }

    macro_rules! int_impl {
        ($ity:ty, $uty:ty, $bits:expr) => {
            impl Int for $uty {
                type OtherSign = $ity;
                type UnsignedInt = $uty;

                fn extract_sign(self) -> (bool, $uty) {
                    (false, self)
                }

                fn unsigned(self) -> $uty {
                    self
                }

                fn from_unsigned(me: $uty) -> Self {
                    me
                }

                int_impl_common!($uty, $bits);
            }

            impl Int for $ity {
                type OtherSign = $uty;
                type UnsignedInt = $uty;

                fn extract_sign(self) -> (bool, $uty) {
                    if self < 0 {
                        (true, (!(self as $uty)).wrapping_add(1))
                    } else {
                        (false, self as $uty)
                    }
                }

                fn unsigned(self) -> $uty {
                    self as $uty
                }

                fn from_unsigned(me: $uty) -> Self {
                    me as $ity
                }

                int_impl_common!($ity, $bits);
            }
        };
    }

    int_impl!(i16, u16, 16);
    int_impl!(i32, u32, 32);
    int_impl!(i64, u64, 64);
    int_impl!(i128, u128, 128);



    /// Trait to express (possibly lossy) casting of integers
    pub(crate) trait CastInto<T: Copy>: Copy {
        fn cast(self) -> T;
    }

    macro_rules! cast_into {
        ($ty:ty) => {
            cast_into!($ty; usize, isize, u32, i32, u64, i64, u128, i128);
        };
        ($ty:ty; $($into:ty),*) => {$(
            impl CastInto<$into> for $ty {
                fn cast(self) -> $into {
                    self as $into
                }
            }
        )*};
    }

    cast_into!(u32);
    cast_into!(i32);
    cast_into!(u64);
    cast_into!(i64);
    cast_into!(u128);
    cast_into!(i128);
}
