//! This module is used to simplify importing the most common UEFI types.
//!
//! This includes the system table types, `Status` codes, etc.

pub use crate::{ResultExt, ResultExt2, Status};

// Import the basic table types.
pub use crate::table::boot::BootServices;
pub use crate::table::runtime::RuntimeServices;
pub use crate::table::{Boot, SystemTable};
