//! This module provides tooling that facilitates dealing with C-style enums
//!
//! C-style enums and Rust-style enums are quite different. There are things
//! which one allows, but not the other, and vice versa. In an FFI context, two
//! aspects of C-style enums are particularly bothersome to us:
//!
//! - They allow a caller to send back an unknown enum variant. In Rust, the
//!   mere act of storing such a variant in a variable is undefined behavior.
//! - They have an implicit conversion to integers, which is often used as a
//!   more portable alternative to C bitfields or as a way to count the amount
//!   of variants of an enumerated type. Rust enums do not model this well.
//!
//! Therefore, in many cases, C enums are best modeled as newtypes of integers
//! featuring a large set of associated constants instead of as Rust enums. This
//! module provides facilities to simplify this kind of FFI.

/// Interface a C-style enum as an integer newtype.
///
/// This macro implements Debug for you, the way you would expect it to work on
/// Rust enums (printing the variant name instead of its integer value). It also
/// derives Clone, Copy, Eq and PartialEq, since that always makes sense for
/// C-style enums and is used by the implementation. If you want anything else
/// to be derived, you can ask for it by adding extra derives as shown in the
/// example below.
///
/// One minor annoyance is that since variants will be translated into
/// associated constants in a separate impl block, you need to discriminate
/// which attributes should go on the type and which should go on the impl
/// block. The latter should go on the right-hand side of the arrow operator.
///
/// Usage example:
/// ```
/// newtype_enum! {
/// #[derive(Cmp, PartialCmp)]
/// pub enum UnixBool: i32 => #[allow(missing_docs)] {
///     FALSE          =  0,
///     TRUE           =  1,
///     /// Nobody expects the Unix inquisition!
///     FILE_NOT_FOUND = -1,
/// }}
/// ```
#[macro_export]
macro_rules! newtype_enum {
    (
        $(#[$type_attrs:meta])*
        pub enum $type:ident : $base_integer:ty => $(#[$impl_attrs:meta])* {
            $(
                $(#[$variant_attrs:meta])*
                $variant:ident = $value:expr,
            )*
        }
    ) => {
        $(#[$type_attrs])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Eq, PartialEq)]
        pub struct $type(pub $base_integer);

        $(#[$impl_attrs])*
        #[allow(unused)]
        impl $type {
            $(
                $(#[$variant_attrs])*
                pub const $variant: $type = $type($value);
            )*
        }

        impl core::fmt::Debug for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                match *self {
                    // Display variants by their name, like Rust enums do
                    $(
                        $type::$variant => write!(f, stringify!($variant)),
                    )*

                    // Display unknown variants in tuple struct format
                    $type(unknown) => {
                        write!(f, "{}({})", stringify!($type), unknown)
                    }
                }
            }
        }
    }
}
