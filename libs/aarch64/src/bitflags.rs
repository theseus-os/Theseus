// Copy of https://github.com/tamird/bitflags/blob/8bb7e6252f3b09b0317359c3042b9d4095948bf8/src/lib.rs
// (associated-constants branch)

// Copyright 2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! A typesafe bitmask flag generator (tamird's associated-constants branch)

macro_rules! bitflags {
    ($(#[$attr:meta])* pub flags $BitFlags:ident: $T:ty {
        $($(#[$Flag_attr:meta])* const $Flag:ident = $value:expr),+
    }) => {
        #[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord, Hash)]
        $(#[$attr])*
        pub struct $BitFlags {
            bits: $T,
        }

        bitflags! {
            @_impl flags $BitFlags: $T {
                $($(#[$Flag_attr])* const $Flag = $value),+
            }
        }
    };
    ($(#[$attr:meta])* flags $BitFlags:ident: $T:ty {
        $($(#[$Flag_attr:meta])* const $Flag:ident = $value:expr),+
    }) => {
        #[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord, Hash)]
        $(#[$attr])*
        struct $BitFlags {
            bits: $T,
        }

        bitflags! {
            @_impl flags $BitFlags: $T {
                $($(#[$Flag_attr])* const $Flag = $value),+
            }
        }
    };
    (@_impl flags $BitFlags:ident: $T:ty {
        $($(#[$Flag_attr:meta])* const $Flag:ident = $value:expr),+
    }) => {
        impl $crate::core::fmt::Debug for $BitFlags {
            #[allow(unused_assignments)]
            fn fmt(&self, f: &mut $crate::core::fmt::Formatter) -> $crate::core::fmt::Result {
                // This convoluted approach is to handle #[cfg]-based flag
                // omission correctly. Some of the $Flag variants may not be
                // defined in this module so we create a trait which defines
                // *all* flags to the value of 0. Afterwards we define a struct
                // which overrides the definitions in the trait for all defined
                // variants, leaving only the undefined ones with the bit value
                // of 0.
                trait DummyTrait {
                    // Now we define the "undefined" versions of the flags.
                    // This way, all the names exist, even if some are #[cfg]ed
                    // out.
                    $(const $Flag: $BitFlags = $BitFlags { bits: 0 };)+
                };
                struct DummyStruct;
                impl DummyTrait for DummyStruct {
                    // Then we override the "undefined" flags with flags that
                    // have their proper value. Flags that are #[cfg]ed out
                    // retain the default value of 0 defined by the trait.
                    $($(#[$Flag_attr])* const $Flag: $BitFlags = $BitFlags { bits: $value };)+
                };
                let mut first = true;
                $(
                    // $Flag.bits == 0 means that $Flag doesn't exist
                    if DummyStruct::$Flag.bits != 0 && self.contains(DummyStruct::$Flag) {
                        if !first {
                            try!(f.write_str(" | "));
                        }
                        first = false;
                        try!(f.write_str(stringify!($Flag)));
                    }
                )+
                Ok(())
            }
        }

        #[allow(dead_code)]
        impl $BitFlags {
            $($(#[$Flag_attr])* pub const $Flag: $BitFlags = $BitFlags { bits: $value };)+

            /// Returns an empty set of flags.
            #[inline]
            pub fn empty() -> $BitFlags {
                $BitFlags { bits: 0 }
            }

            /// Returns the set containing all flags.
            #[inline]
            pub fn all() -> $BitFlags {
                // See above for why this approach is taken.
                trait DummyTrait {
                    $(const $Flag: $BitFlags = $BitFlags { bits: 0 };)+
                };
                struct DummyStruct;
                impl DummyTrait for DummyStruct {
                    $($(#[$Flag_attr])* const $Flag: $BitFlags = $BitFlags { bits: $value };)+
                };
                $BitFlags { bits: $(DummyStruct::$Flag.bits)|+ }
            }

            /// Returns the raw value of the flags currently stored.
            #[inline]
            pub fn bits(&self) -> $T {
                self.bits
            }

            /// Convert from underlying bit representation, unless that
            /// representation contains bits that do not correspond to a flag.
            #[inline]
            pub fn from_bits(bits: $T) -> $crate::core::option::Option<$BitFlags> {
                if (bits & !$BitFlags::all().bits()) == 0 {
                    $crate::core::option::Option::Some($BitFlags { bits: bits })
                } else {
                    $crate::core::option::Option::None
                }
            }

            /// Convert from underlying bit representation, dropping any bits
            /// that do not correspond to flags.
            #[inline]
            pub fn from_bits_truncate(bits: $T) -> $BitFlags {
                $BitFlags { bits: bits } & $BitFlags::all()
            }

            /// Returns `true` if no flags are currently stored.
            #[inline]
            pub fn is_empty(&self) -> bool {
                *self == $BitFlags::empty()
            }

            /// Returns `true` if all flags are currently set.
            #[inline]
            pub fn is_all(&self) -> bool {
                *self == $BitFlags::all()
            }

            /// Returns `true` if there are flags common to both `self` and `other`.
            #[inline]
            pub fn intersects(&self, other: $BitFlags) -> bool {
                !(*self & other).is_empty()
            }

            /// Returns `true` all of the flags in `other` are contained within `self`.
            #[inline]
            pub fn contains(&self, other: $BitFlags) -> bool {
                (*self & other) == other
            }

            /// Inserts the specified flags in-place.
            #[inline]
            pub fn insert(&mut self, other: $BitFlags) {
                self.bits |= other.bits;
            }

            /// Removes the specified flags in-place.
            #[inline]
            pub fn remove(&mut self, other: $BitFlags) {
                self.bits &= !other.bits;
            }

            /// Toggles the specified flags in-place.
            #[inline]
            pub fn toggle(&mut self, other: $BitFlags) {
                self.bits ^= other.bits;
            }
        }

        impl $crate::core::ops::BitOr for $BitFlags {
            type Output = $BitFlags;

            /// Returns the union of the two sets of flags.
            #[inline]
            fn bitor(self, other: $BitFlags) -> $BitFlags {
                $BitFlags { bits: self.bits | other.bits }
            }
        }

        impl $crate::core::ops::BitOrAssign for $BitFlags {

            /// Adds the set of flags.
            #[inline]
            fn bitor_assign(&mut self, other: $BitFlags) {
                self.bits |= other.bits;
            }
        }

        impl $crate::core::ops::BitXor for $BitFlags {
            type Output = $BitFlags;

            /// Returns the left flags, but with all the right flags toggled.
            #[inline]
            fn bitxor(self, other: $BitFlags) -> $BitFlags {
                $BitFlags { bits: self.bits ^ other.bits }
            }
        }

        impl $crate::core::ops::BitXorAssign for $BitFlags {

            /// Toggles the set of flags.
            #[inline]
            fn bitxor_assign(&mut self, other: $BitFlags) {
                self.bits ^= other.bits;
            }
        }

        impl $crate::core::ops::BitAnd for $BitFlags {
            type Output = $BitFlags;

            /// Returns the intersection between the two sets of flags.
            #[inline]
            fn bitand(self, other: $BitFlags) -> $BitFlags {
                $BitFlags { bits: self.bits & other.bits }
            }
        }

        impl $crate::core::ops::BitAndAssign for $BitFlags {

            /// Disables all flags disabled in the set.
            #[inline]
            fn bitand_assign(&mut self, other: $BitFlags) {
                self.bits &= other.bits;
            }
        }

        impl $crate::core::ops::Sub for $BitFlags {
            type Output = $BitFlags;

            /// Returns the set difference of the two sets of flags.
            #[inline]
            fn sub(self, other: $BitFlags) -> $BitFlags {
                $BitFlags { bits: self.bits & !other.bits }
            }
        }

        impl $crate::core::ops::SubAssign for $BitFlags {

            /// Disables all flags enabled in the set.
            #[inline]
            fn sub_assign(&mut self, other: $BitFlags) {
                self.bits &= !other.bits;
            }
        }

        impl $crate::core::ops::Not for $BitFlags {
            type Output = $BitFlags;

            /// Returns the complement of this set of flags.
            #[inline]
            fn not(self) -> $BitFlags {
                $BitFlags { bits: !self.bits } & $BitFlags::all()
            }
        }

        impl $crate::core::iter::Extend<$BitFlags> for $BitFlags {
            fn extend<T: $crate::core::iter::IntoIterator<Item=$BitFlags>>(&mut self, iterator: T) {
                for item in iterator {
                    self.insert(item)
                }
            }
        }

        impl $crate::core::iter::FromIterator<$BitFlags> for $BitFlags {
            fn from_iter<T: $crate::core::iter::IntoIterator<Item=$BitFlags>>(iterator: T) -> $BitFlags {
                let mut result = Self::empty();
                result.extend(iterator);
                result
            }
        }
    };
    ($(#[$attr:meta])* pub flags $BitFlags:ident: $T:ty {
        $($(#[$Flag_attr:meta])* const $Flag:ident = $value:expr),+,
    }) => {
        bitflags! {
            $(#[$attr])*
            pub flags $BitFlags: $T {
                $($(#[$Flag_attr])* const $Flag = $value),+
            }
        }
    };
    ($(#[$attr:meta])* flags $BitFlags:ident: $T:ty {
        $($(#[$Flag_attr:meta])* const $Flag:ident = $value:expr),+,
    }) => {
        bitflags! {
            $(#[$attr])*
            flags $BitFlags: $T {
                $($(#[$Flag_attr])* const $Flag = $value),+
            }
        }
    };
}
