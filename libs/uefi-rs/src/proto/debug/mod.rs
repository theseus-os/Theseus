//! Provides support for the UEFI debugging protocol.
//!
//! This protocol is designed to allow debuggers to query the state of the firmware,
//! as well as set up callbacks for various events.
//!
//! It also defines a Debugport protocol for debugging over serial devices.
//!
//! An example UEFI debugger is Intel's [UDK Debugger Tool][udk].
//!
//! [udk]: https://firmware.intel.com/develop/intel-uefi-tools-and-utilities/intel-uefi-development-kit-debugger-tool

use crate::proto::Protocol;
use crate::unsafe_guid;

/// The debugging support protocol allows debuggers to connect to a UEFI machine.
#[repr(C)]
#[unsafe_guid("2755590c-6f3c-42fa-9ea4-a3ba543cda25")]
#[derive(Protocol)]
pub struct DebugSupport {
    isa: ProcessorArch,
    // FIXME: Add the mising parts of the interface. Beware that it features
    //        unsafety in the form of an unchecked processor index.
}

impl DebugSupport {
    /// Returns the processor architecture of the running CPU.
    pub fn arch(&self) -> ProcessorArch {
        self.isa
    }
}

newtype_enum! {
/// The instruction set architecture of the running processor.
///
/// UEFI can be and has been ported to new CPU architectures in the past,
/// therefore modeling this C enum as a Rust enum (where the compiler must know
/// about every variant in existence) would _not_ be safe.
pub enum ProcessorArch: u32 => {
    /// 32-bit x86 PC
    X86_32      = 0x014C,
    /// 64-bit x86 PC
    X86_64      = 0x8664,
    /// Intel Itanium
    ITANIUM     = 0x200,
    /// UEFI Interpreter bytecode
    EBC         = 0x0EBC,
    /// ARM Thumb / Mixed
    ARM         = 0x01C2,
    /// ARM 64-bit
    AARCH_64    = 0xAA64,
    /// RISC-V 32-bit
    RISCV_32    = 0x5032,
    /// RISC-V 64-bit
    RISCV_64    = 0x5064,
    /// RISC-V 128-bit
    RISCV_128   = 0x5128,
}}
