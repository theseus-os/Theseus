use core::fmt;

/// A globally unique identifier
///
/// GUIDs are used by UEFI to identify protocols and other objects. They are
/// mostly like variant 2 UUIDs as specified by RFC 4122, but differ from them
/// in that the first 3 fields are little endian instead of big endian.
///
/// The `Display` formatter prints GUIDs in the canonical format defined by
/// RFC 4122, which is also used by UEFI.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(C)]
pub struct Guid {
    /// The low field of the timestamp.
    a: u32,
    /// The middle field of the timestamp.
    b: u16,
    /// The high field of the timestamp multiplexed with the version number.
    c: u16,
    /// Contains, in this order:
    /// - The high field of the clock sequence multiplexed with the variant.
    /// - The low field of the clock sequence.
    /// - The spatially unique node identifier.
    d: [u8; 8],
}

impl Guid {
    /// Creates a new GUID from its canonical representation
    //
    // FIXME: An unwieldy array of bytes must be used for the node ID until one
    //        can assert that an u64 has its high 16-bits cleared in a const fn.
    //        Once that is done, we can take an u64 to be even closer to the
    //        canonical UUID/GUID format.
    //
    pub const fn from_values(
        time_low: u32,
        time_mid: u16,
        time_high_and_version: u16,
        clock_seq_and_variant: u16,
        node: [u8; 6],
    ) -> Self {
        Guid {
            a: time_low,
            b: time_mid,
            c: time_high_and_version,
            d: [
                (clock_seq_and_variant / 0x100) as u8,
                (clock_seq_and_variant % 0x100) as u8,
                node[0],
                node[1],
                node[2],
                node[3],
                node[4],
                node[5],
            ],
        }
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let d = {
            let (low, high) = (u16::from(self.d[0]), u16::from(self.d[1]));

            (low << 8) | high
        };

        // Extract and reverse byte order.
        let e = self.d[2..8].iter().enumerate().fold(0, |acc, (i, &elem)| {
            acc | {
                let shift = (5 - i) * 8;
                u64::from(elem) << shift
            }
        });

        write!(
            fmt,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            self.a, self.b, self.c, d, e
        )
    }
}

/// Several entities in the UEFI specification can be referred to by their GUID,
/// this trait is a building block to interface them in uefi-rs.
///
/// You should never need to use the `Identify` trait directly, but instead go
/// for more specific traits such as `Protocol` or `FileProtocolInfo`, which
/// indicate in which circumstances an `Identify`-tagged type should be used.
///
/// Implementing `Identify` is unsafe because attaching an incorrect GUID to a
/// type can lead to type unsafety on both the Rust and UEFI side.
///
/// You can derive `Identify` for a type using the `unsafe_guid` procedural
/// macro, which is exported by this module. This macro mostly works like a
/// custom derive, but also supports type aliases. It takes a GUID in canonical
/// textual format as an argument, and is used in the following way:
///
/// ```
/// #[unsafe_guid("12345678-9abc-def0-1234-56789abcdef0")]
/// type Emptiness = ();
/// ```
pub unsafe trait Identify {
    /// Unique protocol identifier.
    const GUID: Guid;
}

pub use uefi_macros::unsafe_guid;
