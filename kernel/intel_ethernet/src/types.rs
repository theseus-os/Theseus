//! Type aliases for 32-bit registers that are used to initialize receive and transmit queues.

/// Receive Descriptor Base Address Low Register
pub type Rdbal  = u32;
/// Receive Descriptor Base Address High Register
pub type Rdbah  = u32;
/// Receive Descriptor Length Register
pub type Rdlen  = u32;
/// Receive Descriptor Tail Register
pub type Rdt    = u32;
/// Receive Descriptor Head Register
pub type Rdh    = u32;
/// Transmit Descriptor Base Address Low Register
pub type Tdbal  = u32;
/// Transmit Descriptor Base Address High Register
pub type Tdbah  = u32;
/// Transmit Descriptor Length Register
pub type Tdlen  = u32;
/// Transmit Descriptor Tail Register
pub type Tdt    = u32;
/// Transmit Descriptor Head Register
pub type Tdh    = u32;

