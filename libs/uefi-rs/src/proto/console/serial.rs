//! Abstraction over byte stream devices, also known as serial I/O devices.

use crate::proto::Protocol;
use crate::{unsafe_guid, Result, Status};
use bitflags::bitflags;

/// Provides access to a serial I/O device.
///
/// This can include standard UART devices, serial ports over a USB interface,
/// or any other character-based communication device.
///
/// Since UEFI drivers are implemented through polling, if you fail to regularly
/// check for input/output, some data might be lost.
#[repr(C)]
#[unsafe_guid("bb25cf6f-f1d4-11d2-9a0c-0090273fc1fd")]
#[derive(Protocol)]
pub struct Serial<'boot> {
    // Revision of this protocol, only 1.0 is currently defined.
    // Future versions will be backwards compatible.
    revision: u32,
    reset: extern "win64" fn(&mut Serial) -> Status,
    set_attributes: extern "win64" fn(
        &Serial,
        baud_rate: u64,
        receive_fifo_depth: u32,
        timeout: u32,
        parity: Parity,
        data_bits: u8,
        stop_bits_type: StopBits,
    ) -> Status,
    set_control_bits: extern "win64" fn(&mut Serial, ControlBits) -> Status,
    get_control_bits: extern "win64" fn(&Serial, &mut ControlBits) -> Status,
    write: unsafe extern "win64" fn(&mut Serial, &mut usize, *const u8) -> Status,
    read: unsafe extern "win64" fn(&mut Serial, &mut usize, *mut u8) -> Status,
    io_mode: &'boot IoMode,
}

impl<'boot> Serial<'boot> {
    /// Reset the device.
    pub fn reset(&mut self) -> Result {
        (self.reset)(self).into()
    }

    /// Returns the current I/O mode.
    pub fn io_mode(&self) -> &IoMode {
        self.io_mode
    }

    /// Sets the device's new attributes.
    ///
    /// The given `IoMode` will become the device's new `IoMode`,
    /// with some exceptions:
    ///
    /// - `control_mask` is ignored, since it's a read-only field;
    ///
    /// - values set to `0` / `Default` will be filled with the device's
    ///   default parameters
    ///
    /// - if either `baud_rate` or `receive_fifo_depth` is less than
    ///   the device's minimum, an error will be returned;
    ///   this value will be rounded down to the nearest value supported by the device;
    pub fn set_attributes(&mut self, mode: &IoMode) -> Result {
        (self.set_attributes)(
            self,
            mode.baud_rate,
            mode.receive_fifo_depth,
            mode.timeout,
            mode.parity,
            mode.data_bits as u8,
            mode.stop_bits,
        )
        .into()
    }

    /// Retrieve the device's current control bits.
    pub fn get_control_bits(&self) -> Result<ControlBits> {
        let mut bits = ControlBits::empty();
        (self.get_control_bits)(self, &mut bits).into_with_val(|| bits)
    }

    /// Sets the device's new control bits.
    ///
    /// Not all bits can be modified with this function. A mask of the allowed
    /// bits is stored in the [`ControlBits::SETTABLE`] constant.
    pub fn set_control_bits(&mut self, bits: ControlBits) -> Result {
        (self.set_control_bits)(self, bits).into()
    }

    /// Reads data from this device.
    ///
    /// This operation will block until the buffer has been filled with data or
    /// an error occurs. In the latter case, the error will indicate how many
    /// bytes were actually read from the device.
    pub fn read(&mut self, data: &mut [u8]) -> Result<(), usize> {
        let mut buffer_size = data.len();
        unsafe { (self.read)(self, &mut buffer_size, data.as_mut_ptr()) }.into_with(
            || debug_assert_eq!(buffer_size, data.len()),
            |_| buffer_size,
        )
    }

    /// Writes data to this device.
    ///
    /// This operation will block until the data has been fully written or an
    /// error occurs. In the latter case, the error will indicate how many bytes
    /// were actually written to the device.
    pub fn write(&mut self, data: &[u8]) -> Result<(), usize> {
        let mut buffer_size = data.len();
        unsafe { (self.write)(self, &mut buffer_size, data.as_ptr()) }.into_with(
            || debug_assert_eq!(buffer_size, data.len()),
            |_| buffer_size,
        )
    }
}

/// Structure representing the device's current parameters.
///
/// The default values for all UART-like devices is:
/// - 115,200 baud
/// - 1 byte receive FIFO
/// - 1'000'000 microsecond timeout
/// - no parity
/// - 8 data bits
/// - 1 stop bit
///
/// The software is responsible for flow control.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct IoMode {
    /// Bitmask of the control bits that this device supports.
    pub control_mask: ControlBits,
    /// If applicable, the number of microseconds to wait before assuming an
    /// operation timed out.
    pub timeout: u32,
    /// Device's baud rate, or 0 if unknown.
    pub baud_rate: u64,
    /// Size in character's of the device's buffer.
    pub receive_fifo_depth: u32,
    /// Number of data bits in each character.
    pub data_bits: u32,
    /// If applicable, the parity that is computed or checked for each character.
    pub parity: Parity,
    /// If applicable, the number of stop bits per character.
    pub stop_bits: StopBits,
}

bitflags! {
    /// The control bits of a device. These are defined in the [RS-232] standard.
    ///
    /// [RS-232]: https://en.wikipedia.org/wiki/RS-232
    pub struct ControlBits: u32 {
        /// Clear to send
        const CLEAR_TO_SEND = 0x10;
        /// Data set ready
        const DATA_SET_READY = 0x20;
        /// Indicates that a phone line is ringing
        const RING_INDICATE = 0x40;
        /// Indicates the connection is still connected
        const CARRIER_DETECT = 0x80;
        /// The input buffer is empty
        const INPUT_BUFFER_EMPTY = 0x100;
        /// The output buffer is empty
        const OUTPUT_BUFFER_EMPTY = 0x200;

        /// Terminal is ready for communications
        const DATA_TERMINAL_READY = 0x1;
        /// Request the device to send data
        const REQUEST_TO_SEND = 0x2;
        /// Enable hardware loop-back
        const HARDWARE_LOOPBACK_ENABLE = 0x1000;
        /// Enable software loop-back
        const SOFTWARE_LOOPBACK_ENABLE = 0x2000;
        /// Allow the hardware to handle flow control
        const HARDWARE_FLOW_CONTROL_ENABLE = 0x4000;

        /// Bitmask of the control bits that can be set.
        ///
        /// Up to date as of UEFI 2.7 / Serial protocol v1
        const SETTABLE =
            ControlBits::DATA_TERMINAL_READY.bits
            | ControlBits::REQUEST_TO_SEND.bits
            | ControlBits::HARDWARE_LOOPBACK_ENABLE.bits
            | ControlBits::SOFTWARE_LOOPBACK_ENABLE.bits
            | ControlBits::HARDWARE_FLOW_CONTROL_ENABLE.bits;
    }
}

/// The parity of the device.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum Parity {
    /// Device default
    Default = 0,
    /// No parity
    None,
    /// Even parity
    Even,
    /// Odd parity
    Odd,
    /// Mark parity
    Mark,
    /// Space parity
    Space,
    // SAFETY: The serial protocol is very old, and new parity modes are very
    //         unlikely to be added at this point in time. Therefore, modeling
    //         this C enum as a Rust enum seems safe.
}

/// Number of stop bits per character.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum StopBits {
    /// Device default
    Default = 0,
    /// 1 stop bit
    One,
    /// 1.5 stop bits
    OneFive,
    /// 2 stop bits
    Two,
    // SAFETY: The serial protocol is very old, and new stop bit modes are very
    //         unlikely to be added at this point in time. Therefore, modeling
    //         this C enum as a Rust enum seems safe.
}
