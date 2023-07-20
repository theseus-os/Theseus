pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Error {
    Exhausted,
    Illegal,
    Unaddressable,
    Finished,
    Truncated,
    Checksum,
    Unrecognized,
    Fragmented,
    Malformed,
    Dropped,
    NotSupported,
    Unknown,
}

// impl From<smoltcp::Error> for Error {
//     fn from(e: smoltcp::Error) -> Self {
//         match e {
//             smoltcp::Error::Exhausted => Self::Exhausted,
//             smoltcp::Error::Illegal => Self::Illegal,
//             smoltcp::Error::Unaddressable => Self::Unaddressable,
//             smoltcp::Error::Finished => Self::Finished,
//             smoltcp::Error::Truncated => Self::Truncated,
//             smoltcp::Error::Checksum => Self::Checksum,
//             smoltcp::Error::Unrecognized => Self::Unrecognized,
//             smoltcp::Error::Fragmented => Self::Fragmented,
//             smoltcp::Error::Malformed => Self::Malformed,
//             smoltcp::Error::Dropped => Self::Dropped,
//             smoltcp::Error::NotSupported => Self::NotSupported,
//             _ => Self::Unknown,
//         }
//     }
// }

// impl From<Error> for smoltcp::Error {
//     fn from(e: Error) -> Self {
//         match e {
//             Error::Exhausted => Self::Exhausted,
//             Error::Illegal => Self::Illegal,
//             Error::Unaddressable => Self::Unaddressable,
//             Error::Finished => Self::Finished,
//             Error::Truncated => Self::Truncated,
//             Error::Checksum => Self::Checksum,
//             Error::Unrecognized => Self::Unrecognized,
//             Error::Fragmented => Self::Fragmented,
//             Error::Malformed => Self::Malformed,
//             Error::Dropped => Self::Dropped,
//             Error::NotSupported => Self::NotSupported,
//             // TODO: Ideally smoltcp::Error would have an unknown variant.
//             Error::Unknown => Self::Illegal,
//         }
//     }
// }

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Error::Exhausted => "buffer space exhausted",
            Error::Illegal => "illegal operation",
            Error::Unaddressable => "unaddressable destination",
            Error::Finished => "operation finished",
            Error::Truncated => "trunacted packet",
            Error::Checksum => "checksum error",
            Error::Unrecognized => "unrecognized packet",
            Error::Fragmented => "fragmented packet",
            Error::Malformed => "malformed packet",
            Error::Dropped => "dropped by socket",
            Error::NotSupported => "not supported by network stack",
            Error::Unknown => "unknown network error",
        })
    }
}
