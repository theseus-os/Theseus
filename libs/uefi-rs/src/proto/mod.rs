//! Protocol definitions.
//!
//! Protocols are sets of related functionality.
//!
//! Protocols are identified by a unique ID.
//!
//! Protocols can be implemented by a UEFI driver,
//! and are usually retrieved from a standard UEFI table or
//! by querying a handle.

use crate::Identify;

/// Common trait implemented by all standard UEFI protocols
///
/// According to the UEFI's specification, protocols are `!Send` (they expect to
/// be run on the bootstrap processor) and `!Sync` (they are not thread-safe).
/// You can derive the `Protocol` trait, add these bounds and specify the
/// protocol's GUID using the following syntax:
///
/// ```
/// #[unsafe_guid("12345678-9abc-def0-1234-56789abcdef0")]
/// #[derive(Protocol)]
/// struct DummyProtocol {}
/// ```
pub trait Protocol: Identify {}

pub use uefi_macros::Protocol;

pub mod console;
pub mod debug;
pub mod media;
