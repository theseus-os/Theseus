//! Driver for the PS/2 interface, which supports legacy keyboard and mouse devices.
//!
//! The [`init()`] function sets up the single system-wide [`PS2Controller`],
//! which can then be used to obtain a reference to a [`PS2Keyboard`] or [`PS2Mouse`].

#![no_std]

use log::{debug, warn};
use modular_bitfield::{specifiers::{B1, B4, B8}, bitfield, BitfieldSpecifier};
use num_enum::TryFromPrimitive;
use port_io::Port;
use spin::{Mutex, Once};
use fadt::Fadt;

use HostToControllerCommand::*;
use HostToKeyboardCommand::*;
use HostToMouseCommand::*;
use DeviceToHostResponse::*;


/// The system-wide singleton PS/2 Controller.
static PS2_CONTROLLER: Once<PS2Controller> = Once::new();


/// Initializes the PS/2 controller, and the first and second PS/2 ports (if they exist).
///
/// We roughly follow the procedure from the [OS dev wiki].
///
/// [OS dev wiki]: https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
pub fn init() -> Result<&'static PS2Controller, &'static str> {
    if PS2_CONTROLLER.is_completed() {
        return Err("The PS/2 Controller has already been initialized once.");
    }

    // Step 1: Initialise USB Controllers
    // no USB support yet

    // Step 2: Determine if the PS/2 Controller Exists.
    // If ACPI, and therefore the FADT, is unsupported, the PS/2 controller is assumed to exist.
    let acpi_tables = acpi::get_acpi_tables().lock();
    if let Some(fadt) = Fadt::get(&acpi_tables) {
        // If earlier than ACPI v2, the PS/2 controller is assumed to exist
        if fadt.header.revision > 1 {
            let has_controller = fadt.iapc_boot_architecture_flags & 0b10 == 0b10;
            if !has_controller {
                return Err("no PS/2 Controller present");
            }
        }
    }

    // Here: the PS/2 Controller exists, so create the object representing it.
    let controller = PS2Controller {
        data_port: Mutex::new(Port::new(PS2Controller::PS2_DATA_PORT)),
        command_and_status_port: Mutex::new(Port::new(PS2Controller::PS2_COMMAND_AND_STATUS_PORT))
    };

    // Step 3: Disable Devices
    controller.write_command(DisablePort1);
    controller.write_command(DisablePort2);

    // Step 4: Flush The Output Buffer
    controller.flush_output_buffer();

    // Step 5: Set the Controller Configuration Byte
    let mut config = controller.read_config();
    config.set_port1_interrupt_enabled(false);
    config.set_port2_interrupt_enabled(false);
    config.set_port1_scancode_translation_enabled(false);
    let has_mouse = config.port2_clock_disabled();
    controller.write_config(config);

    // Step 6: Perform Controller Self Test
    controller.test()?;
    debug!("passed PS/2 controller test");
    // as this can reset the controller on some hardware, we restore the config
    controller.write_config(config);

    // Step 7: Determine If There Are 2 Channels
    // not needed, already done above

    // Step 8: Perform Interface Tests
    let port_1_works = controller.test_port1().is_ok();
    let port_2_works = if has_mouse {
        controller.test_port2().is_ok()
    } else {
        warn!("PS/2 controller is not dual channel, so no mouse available");
        false
    };
    if !port_1_works && !port_2_works {
        return Err("failed both PS/2 port tests, terminating PS/2 controller driver");
    }

    // Step 9: Enable Devices
    if port_1_works {
        debug!("passed PS/2 port 1 test");
        controller.write_command(EnablePort1);
        config.set_port1_clock_disabled(false);
        config.set_port1_interrupt_enabled(true);
    }
    if port_2_works {
        debug!("passed PS/2 port 2 test");
        controller.write_command(EnablePort2);
        config.set_port2_clock_disabled(false);
        config.set_port2_interrupt_enabled(true);
    }
    controller.write_config(config);

    // Step 10: Reset Devices
    controller.keyboard_ref().reset()?;
    controller.mouse_ref().reset()?;

    debug!("Final PS/2 {:?}", controller.read_config());
    
    Ok(PS2_CONTROLLER.call_once(|| controller))
}

/// An initialized PS/2 controller that can be used to communicate with
/// legacy PS/2 keyboard and mouse devices.
pub struct PS2Controller {
    /// at 0x60
    data_port: Mutex<Port<u8>>,
    /// at 0x64
    command_and_status_port: Mutex<Port<u8>>,
}

impl PS2Controller {
    /// Port used to transmit data to and from the PS/2 Controller and its devices.
    const PS2_DATA_PORT: u16 = 0x60;
    /// Port used to send commands to and receive status from the PS/2 Controller.
    const PS2_COMMAND_AND_STATUS_PORT: u16 = 0x64;

    /// Returns a reference to the keyboard attached to this PS/2 controller.
    ///
    /// The keyboard only uses the data port.
    pub fn keyboard_ref(&self) -> PS2Keyboard {
        PS2Keyboard::new(self)
    }

    /// Returns a reference to the mouse attached to this PS/2 controller.
    ///
    /// The mouse uses both the data port and the command/status port.
    pub fn mouse_ref(&self) -> PS2Mouse {
        PS2Mouse::new(self)
    }

    /// Writes a command to the PS/2 command port.
    ///
    /// ## Usage Note
    /// Devices attached to the controller should be disabled *before* writing
    /// a command that returns data, otherwise the output buffer may get overwritten.
    pub fn write_command(&self, value: HostToControllerCommand) {
        unsafe {
            self.command_and_status_port.lock().write(value as u8);
        }
    }

    /// Reads and returns the current config value from the PS/2 controller.
    pub fn read_config(&self) -> ControllerConfigurationByte {
        self.write_command(ReadFromInternalRAMByte0);
        ControllerConfigurationByte::from_bytes([self.read_data()])
    }
    
    /// Writes the given config `value` to the PS/2 controller.
    pub fn write_config(&self, value: ControllerConfigurationByte) {
        self.write_command(WriteToInternalRAMByte0);
        self.write_data(WritableData::Configuration(value));
    }

    /// Reads the PS/2 status port.
    pub fn status_register(&self) -> ControllerToHostStatus {
        ControllerToHostStatus::from_bytes([self.command_and_status_port.lock().read()])
    }

    /// Clean the data port output buffer, skipping the [ControllerToHostStatus] `output_buffer_full` check
    pub fn flush_output_buffer(&self) {
        self.read_data();
    }

    pub fn test(&self) -> Result<(), &'static str> {
        self.write_command(TestController);
        self.read_controller_test_result()
    }

    pub fn test_port1(&self) -> Result<(), &'static str> {
        self.write_command(TestPort1);
        self.match_port_test_result().map_err(|e| {
            warn!("failed PS/2 port 1 test: {e}");
            e
        })
    }

    pub fn test_port2(&self) -> Result<(), &'static str> {
        self.write_command(TestPort2);
        self.match_port_test_result().map_err(|e| {
            warn!("failed PS/2 port 2 test: {e}");
            e
        })
    }

    fn match_port_test_result(&self)-> Result<(), &'static str> {
        use PortTestResult::*;
        match self.read_port_test_result()? {
            Passed => Ok(()),
            ClockLineStuckLow  => Err("clock line stuck low"),
            ClockLineStuckHigh => Err("clock line stuck high"),
            DataLineStuckLow   => Err("data line stuck low"),
            DataLineStuckHigh  => Err("data line stuck high"),
        }
    }

    /// must only be called after writing the [TestController] command
    /// otherwise would read bogus data
    pub fn read_controller_test_result(&self) -> Result<(), &'static str> {
        const CONTROLLER_TEST_PASSED: u8 = 0x55;
        if self.read_data() != CONTROLLER_TEST_PASSED {
            Err("failed PS/2 controller test")
        } else {
            Ok(())
        }
    }

    /// must only be called after writing the [TestPort1] or [TestPort2] command
    /// otherwise would read bogus data
    pub fn read_port_test_result(&self) -> Result<PortTestResult, &'static str> {
        self.read_data().try_into().map_err(|_| "failed to read port test result")
    }

    /// e.g. https://wiki.osdev.org/PS/2_Mouse#Set_Sample_Rate_Example
    fn polling_send_receive(&self, value: HostToDevice) -> Result<DeviceToHostResponse, &'static str> {
        self.polling_send(value)?;
        self.polling_receive()
    }

    /// https://wiki.osdev.org/%228042%22_PS/2_Controller#Sending_Bytes_To_Device.2Fs
    fn polling_send(&self, value: HostToDevice) -> Result<(), &'static str> {
        const VERY_ARBITRARY_TIMEOUT_VALUE: u8 = u8::MAX;
        for _ in 0..VERY_ARBITRARY_TIMEOUT_VALUE {
            // so that we don't overwrite the previously sent value
            if !self.status_register().input_buffer_full() {
                self.write_data(WritableData::HostToDevice(value));
                return Ok(());
            }
        }
        Err("polling_send timeout value exceeded")
    }

    /// https://wiki.osdev.org/%228042%22_PS/2_Controller#Polling, which still has some _maybe relevant information_:
    /// We might still need to disable one of the devices every time we send so we can read_data reliably.
    /// As it currently works, it might only be a problem for older devices, so I won't overoptimize.
    fn polling_receive(&self) -> Result<DeviceToHostResponse, &'static str> {
        const VERY_ARBITRARY_TIMEOUT_VALUE: u8 = u8::MAX;
        for _ in 0..VERY_ARBITRARY_TIMEOUT_VALUE {
            // so that we only read when data is available
            if self.status_register().output_buffer_full() {
                return self.read_data().try_into().map_err(|_| "failed to read device response");
            }
        }
        Err("polling_receive timeout value exceeded")
    }

    /// Read data from the PS/2 data port.
    ///
    /// Note: this can be called directly when within a PS/2 device's interrupt handler.
    /// If polling is used (e.g. in a device's init function), [ControllerToHostStatus] `output_buffer_full` needs to be checked first.
    fn read_data(&self) -> u8 {
        self.data_port.lock().read()
    }

    /// Write data to the PS/2 data port.
    ///
    /// Note: when writing to the PS/2 controller, it might be necessary to check for
    /// [ControllerToHostStatus] `!input_buffer_full` first.
    fn write_data(&self, value: WritableData) {
        unsafe { self.data_port.lock().write(value.into()) };
    }
}

/// A reference to the PS/2 Mouse connected to the PS/2 Controller.
pub struct PS2Mouse<'c> {
    controller: &'c PS2Controller,
    id: MouseId
}

impl<'c> PS2Mouse<'c> {
    /// Create a new reference to the mouse connected to the given PS2 controller.
    ///
    /// The default mouse id of PS/2 is zero.
    pub fn new(controller: &'c PS2Controller) -> Self {
        Self { controller, id: MouseId::Zero }
    }

    /// Reset the mouse.
    pub fn reset(&self) -> Result<(), &'static str> {
        self.command_to_mouse(HostToMouseCommandOrData::MouseCommand(Reset))?; 
        if let Ok(SelfTestPassed) = self.controller.read_data().try_into() {
            //returns mouse id 0
            self.controller.read_data();
            return Ok(());
        }
        Err("failed to reset mouse")
    }

    /// write command to the mouse and handle the result
    fn command_to_mouse(&self, value: HostToMouseCommandOrData) -> Result<(), &'static str> {
        self.controller.write_command(WriteByteToPort2InputBuffer);
        const RETRIES: u8 = 3;
        for _ in 0 ..= RETRIES {
            return match self.controller.polling_send_receive(HostToDevice::Mouse(value.clone()))? {
                Acknowledge => Ok(()),
                ResendCommand => continue,
                _ => Err("wrong response type")
            };
        }
        Err("mouse doesn't support the command or there has been a hardware failure")
    }

    /// set PS/2 mouse's sampling rate
    fn set_mouse_sampling_rate(&self, value: MouseSampleRate) -> Result<(), &'static str> {
        self.command_to_mouse(HostToMouseCommandOrData::MouseCommand(SampleRate))
            .and_then(|_| self.command_to_mouse(HostToMouseCommandOrData::SampleRate(value)))
            .map_err(|_| "failed to set the mouse sampling rate")
    }

    /// set the [MouseId] by magic sequence
    pub fn set_mouse_id(&mut self, id: MouseId) -> Result<(), &'static str> {
        self.disable_mouse_packet_streaming()?;

        use crate::MouseSampleRate::*;
        match id {
            MouseId::Zero => { /* do nothing */ }
            MouseId::Three => {
                for rate in [_200, _100, _80] {
                    self.set_mouse_sampling_rate(rate)?;
                }
            }
            //(maybe) TODO: apparently this needs to check after the first 80 if mouse_id is 3
            MouseId::Four => {
                for rate in [_200, _100, _80, _200, _200, _80] {
                    self.set_mouse_sampling_rate(rate)?;
                }
            }
        }
        self.id = id;

        self.enable_mouse_packet_streaming()?;
        Ok(())
    }

    /// get the [MouseId]
    pub fn mouse_id(&self) -> Result<MouseId, &'static str> {
        self.disable_mouse_packet_streaming()?;

        self.command_to_mouse(HostToMouseCommandOrData::MouseCommand(GetDeviceID))?;
        let id = self.controller
            .read_data()
            .try_into()
            .map_err(|_| "failed to get mouse id: bad response")?;

        self.enable_mouse_packet_streaming()?;
        Ok(id)
    }

    /// resend the most recent packet again
    #[allow(dead_code)]
    fn mouse_resend(&self) -> Result<(), &'static str> {
        self.command_to_mouse(HostToMouseCommandOrData::MouseCommand(HostToMouseCommand::ResendByte))
            .map_err(|_| "failed to resend mouse request")
    }

    /// enable the packet streaming
    fn enable_mouse_packet_streaming(&self) -> Result<(), &'static str> {
        self.command_to_mouse(HostToMouseCommandOrData::MouseCommand(EnableDataReporting)).map_err(|_| {
            "failed to enable mouse streaming"
        })
    }

    /// disable the packet streaming
    fn disable_mouse_packet_streaming(&self) -> Result<(), &'static str> {
        self.command_to_mouse(HostToMouseCommandOrData::MouseCommand(DisableDataReporting)).map_err(|_| {
            "failed to disable mouse streaming"
        })
    }

    /// set the resolution of the mouse
    #[allow(dead_code)]
    fn mouse_resolution(&self, value: MouseResolution) -> Result<(), &'static str> {
        self.command_to_mouse(HostToMouseCommandOrData::MouseCommand(SetResolution))
            .and_then(|_| self.command_to_mouse(HostToMouseCommandOrData::MouseResolution(value)))
            .map_err(|_| "failed to set the mouse resolution")
    }

    /// read the correct [MousePacket] according to [MouseId]
    pub fn read_mouse_packet(&self) -> MousePacket {
        let read_data = || self.controller.read_data();
        match self.id {
            MouseId::Zero => MousePacket::Zero(
                MousePacketGeneric::from_bytes([
                    read_data(), read_data(), read_data()
                ])
            ),
            MouseId::Three => MousePacket::Three(
                MousePacket3::from_bytes([
                    read_data(), read_data(), read_data(), read_data()
                ])
            ),
            MouseId::Four => MousePacket::Four(
                MousePacket4::from_bytes([
                    read_data(), read_data(), read_data(), read_data()
                ])
            ),
        }
    }

    /// Returns `true` if there is content in the PS/2 Mouse's output buffer
    /// that can be read from.
    pub fn is_output_buffer_full(&self) -> bool {
        self.controller.status_register().mouse_output_buffer_full()
    }
}

/// A reference to the PS/2 Keyboard connected to the PS/2 Controller.
pub struct PS2Keyboard<'c> {
    controller: &'c PS2Controller,
}

impl<'c> PS2Keyboard<'c> {
    /// Create a new reference to the keyboard connected to the given PS2 controller.
    pub fn new(controller: &'c PS2Controller) -> Self {
        Self { controller }
    }
    
    /// Reset the keyboard.
    pub fn reset(&self) -> Result<(), &'static str> {
        self.command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(ResetAndStartSelfTest))?; 
        if let Ok(SelfTestPassed) = self.controller.read_data().try_into() {
            return Ok(())
        }
        Err("failed to reset keyboard")
    }

    /// write command to the keyboard and handle the result
    fn command_to_keyboard(&self, value: HostToKeyboardCommandOrData) -> Result<(), &'static str> {
        const RETRIES: u8 = 3;
        for _ in 0..=RETRIES {
            return match self.controller.polling_send_receive(HostToDevice::Keyboard(value.clone()))? {
                Acknowledge => Ok(()),
                ResendCommand => continue,
                _ => Err("wrong response type")
            };
        }
        Err("keyboard doesn't support the command or there has been a hardware failure")
    }

    /// Set the status LEDs on the keyboard.
    pub fn set_keyboard_led(&self, value: LEDState) -> Result<(), &'static str> {
        self.command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(SetLEDStatus))
            .and_then(|_| self.command_to_keyboard(HostToKeyboardCommandOrData::LEDState(value)))
            .map_err(|_| "failed to set the keyboard led")
    }

    /// Set the active scancode set currently used by the keyboard.
    pub fn keyboard_scancode_set(&self, value: ScancodeSet) -> Result<(), &'static str> {
        self.command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(SetScancodeSet))
            .and_then(|_| self.command_to_keyboard(HostToKeyboardCommandOrData::ScancodeSet(value)))
            .map_err(|_| "failed to set the keyboard scancode set")
    }

    /// Detect the [KeyboardType].
    ///
    /// Note:
    /// On the identify command, [KeyboardType::AncientATKeyboard] usually returns no bytes at all, but `PS2Controller::read_data` always returns a byte.
    /// This means `PS2Controller::read_data` would return 0x00 here, even though the value is already reserved for the device type "Standard PS/2 mouse".
    /// As we only care about detecting keyboard types here, it should work.
    pub fn keyboard_detect(&self) -> Result<KeyboardType, &'static str> {
        self.command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(DisableScanning))?;

        self.command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(IdentifyKeyboard))?; 

        let keyboard_type = match self.controller.read_data() {
            0x00 => Ok(KeyboardType::AncientATKeyboard),
            0xAB => {
                match self.controller.read_data() {
                    0x41 | 0xC1 => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
                    0x83 => Ok(KeyboardType::MF2Keyboard),
                    _ => Err("unrecognized keyboard type")
                }
            }
            _ => Err("unrecognized keyboard type")
        };
        self.command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(EnableScanning))?;
        keyboard_type
    }

    /// Reads a raw scancode value from the PS/2 controller's data port.
    pub fn read_scancode(&self) -> u8 {
        self.controller.read_data()
    }
}


// https://wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_Commands
// quite a few of these are commented out because they're either unused, deprecated or non-standard.
/// Commands that can be sent to the PS/2 Controller.
///
/// The naming convention is device-centric:
/// * "Output" means sending data "out of the device to the host (CPU)".
/// * "Input" means sending data "out of the host (CPU) into the device".
///   * Example: [WriteByteToPort2InputBuffer] instructs the host (CPU) to write a byte
///     into port 2 of the device, so the device receives that byte as input.
pub enum HostToControllerCommand {
    /// returns [ControllerConfigurationByte]
    ReadFromInternalRAMByte0 = 0x20,
    
    // return is non-standard
    // /// 0x21-0x3F; N is the command byte & 0x1F
    // ReadFromInternalRAMByteN = 0x21,

    /// sets [ControllerConfigurationByte]
    WriteToInternalRAMByte0 = 0x60,

    // usage is non-standard
    // /// 0x61-0x7F; N is the command byte & 0x1F
    // WriteToInternalRAMByteN = 0x61,

    // these are all deprecated 
    // PasswordInstalledTest = 0xA4,
    // LoadSecurity = 0xA5,
    // EnableSecurity = 0xA6,
    
    /// sets [ControllerConfigurationByte] `port2_clock_disabled`
    ///
    /// Note: only if 2 PS/2 ports supported
    DisablePort2 = 0xA7,
    /// clears [ControllerConfigurationByte] `port2_clock_disabled`
    ///
    /// Note: only if 2 PS/2 ports supported
    EnablePort2 = 0xA8,
    /// returns [PortTestResult]
    ///
    /// Note: only if 2 PS/2 ports supported
    TestPort2 = 0xA9,
    /// see [PS2Controller::test]
    TestController = 0xAA,
    /// returns [PortTestResult]
    TestPort1 = 0xAB,

    // unused
    // /// read all bytes of internal RAM
    // DiagnosticDump = 0xAC,

    /// sets [ControllerConfigurationByte] `port1_clock_disabled`
    DisablePort1 = 0xAD,
    /// clears [ControllerConfigurationByte] `port1_clock_disabled`
    EnablePort1 = 0xAE,

    // return is non-standard
    // ReadControllerInputPort = 0xC0,

    // both unused
    // CopyBits0to3ofInputPortToStatusBits4to7 = 0xC1,
    // CopyBits4to7ofInputPortToStatusBits4to7 = 0xC2,

    // both unused and therefore not implemented, https://wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_Output_Port
    // /// returns [ControllerOutputPort]
    // ReadControllerOutputPort = 0xD0,
    // /// sets [ControllerOutputPort]
    // ///
    // /// Note: Check if the output buffer is empty first
    // WriteByteToControllerOutputPort = 0xD1,

    // both unused
    // /// makes it look like the byte written was received from the first PS/2 port
    // ///
    // /// Note: only if 2 PS/2 ports supported
    // WriteByteToPort1OutputBuffer = 0xD2,
    // /// makes it look like the byte written was received from the second PS/2 port
    // ///
    // /// Note: only if 2 PS/2 ports supported
    // WriteByteToPort2OutputBuffer = 0xD3,

    /// sends next byte to the second PS/2 port
    ///
    /// Note: only if 2 PS/2 ports supported
    WriteByteToPort2InputBuffer = 0xD4,

    // both unused
    // ReadTestInputs = 0xE0,
    // /// 0xF0-0xFF; Bits 0 to 3 correspond to 4 different output lines and are used as a mask:
    // /// 0 = pulse line, 1 = don't pulse line; Bit 0 corresponds to the "reset" line.
    // /// The other output lines don't have a standard/defined purpose.
    // PulseOutputLineLowFor6ms = 0xF0,
}

// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Status_Register
// and https://users.utcluj.ro/~baruch/sie/labor/PS2/PS-2_Keyboard_Interface.htm
#[bitfield(bits = 8)]
pub struct ControllerToHostStatus {
    /// When using [Polling](https://wiki.osdev.org/%228042%22_PS/2_Controller#Polling), must be `true` before attempting to read data from PS/2 data port.
    /// When using [Interrupts](https://wiki.osdev.org/%228042%22_PS/2_Controller#Interrupts), guaranteed to be `true`, because an interrupt only happens if the buffer is full
    /// Alternative name: output_register_full, which might make more sense because it only ever fits 1 byte. "Buffer" sounds like multiple.
    output_buffer_full: bool,
    /// Must be `false` before attempting to write data to PS/2 data port or PS/2 command/status port for 8042 keyboard controller
    /// Alternative name: input_register_full, which might make more sense because it only ever fits 1 byte. "Buffer" sounds like multiple.
    input_buffer_full: bool,
    /// Cleared on reset; set when the system passed Power-on self-test
    #[allow(dead_code)]
    system_passed_self_test: bool,
    /// If `0`, the input buffer should be written to the PS/2 data port.
    /// If `1`, the input buffer should be written to the PS/2 command/status port.
    #[allow(dead_code)]
    input_buffer_is_command: B1,
    /// Whether or not communication is inhibited (via switch) / Unknown (chipset specific) / (more likely unused on modern systems)
    #[allow(dead_code)]
    keyboard_enabled: bool,

    //deprecated (exclusive to AT-compatible mode)
    // /// Keyboard didn't generate clock signals within 15 ms of "request-to-send"
    // transmit_timeout: bool,
    // /// Keyboard didn't generate clock signals within 20 ms of command reception
    // receive_timeout: bool,

    /// Similar to `output_buffer_full`, except for mouse
    pub mouse_output_buffer_full: bool,
    /// Timeout during keyboard command receive or response (Same as `transmit_timeout` + `receive_timeout`)
    #[allow(dead_code)]
    timeout_error: bool,
    /// Should be odd parity, set to `true` if even parity received
    #[allow(dead_code)]
    parity_error: bool,
}

/// Kinds of data that can be written to the PS/2 data port.
enum WritableData {
    Configuration(ControllerConfigurationByte),
    HostToDevice(HostToDevice)
}

impl From<WritableData> for u8 {
    fn from(value: WritableData) -> Self {
        use HostToKeyboardCommandOrData::*;
        use HostToMouseCommandOrData::*;
        match value {
            WritableData::Configuration(value) => u8::from_ne_bytes(value.into_bytes()),
            WritableData::HostToDevice(value) => match value {
                HostToDevice::Keyboard(value) => match value {
                    KeyboardCommand(c) => c as u8,
                    LEDState(l) => u8::from_ne_bytes(l.into_bytes()),
                    ScancodeSet(s) => s as u8,
                }
                HostToDevice::Mouse(value) => match value {
                    MouseCommand(c) => c as u8,
                    MouseResolution(r) => r as u8,
                    SampleRate(s) => s as u8,
                }
            }
        }
    }
}

/// Configuration values used to enable or disable features of the PS/2 controller.
///
/// This type is used with [HostToControllerCommand::ReadFromInternalRAMByte0]
/// and [HostToControllerCommand::WriteToInternalRAMByte0].
///
/// [More info](wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_Configuration_Byte)
#[bitfield(bits = 8)]
#[derive(Debug, Clone, Copy)]
pub struct ControllerConfigurationByte {
    /// interrupt on [ControllerToHostStatus] `output_buffer_full`
    pub port1_interrupt_enabled: bool,
    /// interrupt on [ControllerToHostStatus] `mouse_output_buffer_full`
    ///
    /// Note: only if 2 PS/2 ports supported
    pub port2_interrupt_enabled: bool,
    /// Cleared on reset; set when the system passed Power-on self-test
    #[allow(dead_code)]
    system_passed_self_test: bool,
    // or override_keyboard_inhibiting
    #[allow(dead_code)]
    should_be_zero: B1,
    /// disables the keyboard
    pub port1_clock_disabled: bool,
    /// disables the auxilary device (mouse)
    ///
    /// Note: only if 2 PS/2 ports supported
    pub port2_clock_disabled: bool,
    /// whether IBM scancode translation is enabled (0=AT, 1=PC)
    pub port1_scancode_translation_enabled: bool,
    #[allow(dead_code)]
    must_be_zero: B1,
}

#[derive(TryFromPrimitive)]
#[repr(u8)]
pub enum PortTestResult {
    Passed = 0x00,
    ClockLineStuckLow = 0x01,
    ClockLineStuckHigh = 0x02,
    DataLineStuckLow = 0x03,
    DataLineStuckHigh = 0x04,
}

// https://wiki.osdev.org/PS/2_Keyboard#Special_Bytes all other bytes sent by the keyboard are scan codes
// http://users.utcluj.ro/~baruch/sie/labor/PS2/PS-2_Mouse_Interface.htm mouse only returns AA, FC or FA, mouse_id and packet
#[derive(TryFromPrimitive)]
#[repr(u8)]
pub enum DeviceToHostResponse {
    KeyDetectionErrorOrInternalBufferOverrun1 = 0x00,
    /// sent after "0xFF (reset)" command or keyboard power up
    SelfTestPassed = 0xAA,
    ResponseToEcho = 0xEE,
    Acknowledge = 0xFA,
    /// sent after "0xFF (reset)" command or keyboard power up
    SelfTestFailed1 = 0xFC,
    /// sent after "0xFF (reset)" command or keyboard power up
    SelfTestFailed2 = 0xFD,
    ResendCommand = 0xFE,
    KeyDetectionErrorOrInternalBufferOverrun2 = 0xFF,
}

#[derive(Clone)]
enum HostToDevice {
    Keyboard(HostToKeyboardCommandOrData),
    Mouse(HostToMouseCommandOrData)
}

// https://wiki.osdev.org/PS/2_Keyboard#Commands
#[derive(Clone)]
pub enum HostToKeyboardCommandOrData {
    KeyboardCommand(HostToKeyboardCommand),
    LEDState(LEDState),
    ScancodeSet(ScancodeSet),
    //TODO: Typematic, Scancode
}

#[derive(Clone)]
pub enum HostToKeyboardCommand {
    SetLEDStatus = 0xED,
    // unused
    // /// for diagnostic purposes and useful for device removal detection
    // Echo = 0xEE,
    SetScancodeSet = 0xF0,
    IdentifyKeyboard = 0xF2,
    /// also called typematic
    SetRepeatRateAndDelay = 0xF3,
    EnableScanning = 0xF4,
    /// might also restore default parameters
    DisableScanning = 0xF5,
    // unused
    //SetDefaultParameters = 0xF6,

    // unused and scancode set 3 only
    //SetAllKeysToAutorepeat = 0xF7,
    //SetAllKeysToMakeRelease = 0xF8,
    //SetAllKeysToMake = 0xF9,
    //SetAllKeysToAutorepeatMakeRelease = 0xFA,
    //SetKeyToAutorepeat = 0xFB,
    //SetKeyToMakeRelease = 0xFC,
    //SetKeyToMake = 0xFD,

    ResendByte = 0xFE,
    ResetAndStartSelfTest = 0xFF,
}

#[bitfield(bits = 3)]
#[derive(Clone)]
pub struct LEDState {
    pub scroll_lock: bool,
    pub number_lock: bool,
    pub caps_lock: bool,
}

#[derive(Clone)]
pub enum ScancodeSet {
    // GetCurrentSet = 0, //TODO, if needed
    Set1 = 1,
    Set2 = 2,
    Set3 = 3,
}

#[derive(Clone)]
pub enum HostToMouseCommandOrData {
    MouseCommand(HostToMouseCommand),
    SampleRate(MouseSampleRate),
    MouseResolution(MouseResolution),
}

// https://wiki.osdev.org/PS/2_Mouse#Mouse_Device_Over_PS.2F2
// the comments right beside the fields are just there for some intuition on PS/2
#[derive(Clone)]
pub enum HostToMouseCommand {
    /// either to 1 or 2
    SetScaling = 0xE6,
    /// set [MouseResolution]
    SetResolution = 0xE8,
    StatusRequest = 0xE9,
    SetStreamMode = 0xEA,
    ReadData = 0xEB,
    ResetWrapMode = 0xEC,
    SetWrapMode = 0xEE,
    SetRemoteMode = 0xF0, //same value as SetScancodeSet
    GetDeviceID = 0xF2, //same value as IdentifyKeyboard
    SampleRate = 0xF3,  //same value as SetRepeatRateAndDelay
    EnableDataReporting = 0xF4, //same value as EnableScanning
    DisableDataReporting = 0xF5, //same value as DisableScanning
    SetDefaults = 0xF6, //same
    ResendByte = 0xFE,  //same
    Reset = 0xFF,       //same
}

#[bitfield(bits = 24)]
#[derive(Debug, BitfieldSpecifier)]
pub struct MousePacketGeneric {
    //1. byte starts here
    pub button_left: bool,
    pub button_right: bool,
    pub button_middle: bool,
    pub always_one: B1,
    /// see [x_1st_to_8th_bit]
    x_9th_bit: bool,
    /// see [y_1st_to_8th_bit]
    y_9th_bit: bool,
    x_overflow: bool,
    y_overflow: bool,
    //2. byte
    /// only a part of x_movement, needs to be combined with [x_9th_bit]
    x_1st_to_8th_bit: B8,
    //3. byte
    /// only a part of y_movement, needs to be combined with [y_9th_bit]
    y_1st_to_8th_bit: B8,
}

impl MousePacketGeneric {
    /// `x_1st_to_8th_bit` and `x_9th_bit` should not be accessed directly, because they're part of one signed 9-bit number
    ///
    /// Note: This handles overflow as well, although it might not happen on newer systems <https://forum.osdev.org/viewtopic.php?f=1&t=31176>
    pub fn x_movement(&self) -> i16 {
        let x_1st_to_8th_bit = if self.x_overflow() {
            u8::MAX
        } else {
            self.x_1st_to_8th_bit()
        };
        Self::to_2s_complement(x_1st_to_8th_bit, self.x_9th_bit())
    }

    /// `y_1st_to_8th_bit` and `y_9th_bit` should not be accessed directly, because they're part of one signed 9-bit number
    ///
    /// Note: This handles overflow as well, although it might not happen on newer systems <https://forum.osdev.org/viewtopic.php?f=1&t=31176>
    pub fn y_movement(&self) -> i16 {
        let y_1st_to_8th_bit = if self.y_overflow() {
            u8::MAX
        } else {
            self.y_1st_to_8th_bit()
        };
        Self::to_2s_complement(y_1st_to_8th_bit, self.y_9th_bit())
    }

    // implementation from https://wiki.osdev.org/PS/2_Mouse
    fn to_2s_complement(bit1to8: u8, bit9: bool) -> i16 {
        // to fit a 9th bit inside; we can't just convert to i16, because it would turn e.g. 255 into -1
        let unsigned = bit1to8 as u16; // 1111_1111 as u16 = 0000_0000_1111_1111

        // now convert into i16, which always gives us a positive number
        let signed = unsigned as i16; // 0000_0000_1111_1111 as i16 = 0000_0000_1111_1111
        
        if bit9 {
            // value is negative, produce the two's complement, correctly sign extended no matter its size
            signed - 0b1_0000_0000 // 0000_0000_1111_1111 - 1_0000_0000 = -1
        } else {
            signed
        }
    }
}

#[bitfield(bits = 32)]
#[derive(Debug)]
pub struct MousePacket3 {
    generic_part: MousePacketGeneric,
    //4. byte
    /// already stored in two's complement, valid values are -8 to +7
    z_movement: u8,
}
impl MousePacket3 {
    //TODO: see osdev wiki, might be wrong
    /// often called `z_movement`, renamed to disambiguate
    pub fn scroll_movement(&self) -> i8 {
        self.z_movement() as i8
    }
}

// see https://wiki.osdev.org/PS/2_Mouse#5_buttons
#[bitfield(bits = 32)]
#[derive(Debug)]
pub struct MousePacket4 {
    generic_part: MousePacketGeneric,
    //4. byte starts here
    /// already stored in two's complement
    z_movement: B4,
    pub button_4: bool,
    pub button_5: bool,
    #[allow(dead_code)]
    zero1: B1,
    #[allow(dead_code)]
    zero2: B1,
}
impl MousePacket4 {
    //TODO: see osdev wiki, might be wrong
    /// often called `z_movement`, renamed to disambiguate
    pub fn scroll_movement(&self) -> i8 {
        self.z_movement() as i8
    }
}

#[derive(Debug)]
pub enum MousePacket {
    Zero(MousePacketGeneric),
    Three(MousePacket3),
    Four(MousePacket4)
}

// demultiplexes [MousePacket] and provides standard values
impl MousePacket {
    pub fn button_left(&self) -> bool {
        match self {
            MousePacket::Zero(m) => m.button_left(),
            MousePacket::Three(m) => m.generic_part().button_left(),
            MousePacket::Four(m) => m.generic_part().button_left(),
        }
    }
    pub fn button_right(&self) -> bool {
        match self {
            MousePacket::Zero(m) => m.button_right(),
            MousePacket::Three(m) => m.generic_part().button_right(),
            MousePacket::Four(m) => m.generic_part().button_right(),
        }
    }
    pub fn button_middle(&self) -> bool {
        match self {
            MousePacket::Zero(m) => m.button_middle(),
            MousePacket::Three(m) => m.generic_part().button_middle(),
            MousePacket::Four(m) => m.generic_part().button_middle(),
        }
    }
    pub fn always_one(&self) -> u8 {
        match self {
            MousePacket::Zero(m) => m.always_one(),
            MousePacket::Three(m) => m.generic_part().always_one(),
            MousePacket::Four(m) => m.generic_part().always_one(),
        }
    }
    pub fn x_movement(&self) -> i16 {
        match self {
            MousePacket::Zero(m) => m.x_movement(),
            MousePacket::Three(m) => m.generic_part().x_movement(),
            MousePacket::Four(m) => m.generic_part().x_movement(),
        }
    }
    pub fn y_movement(&self) -> i16 {
        match self {
            MousePacket::Zero(m) => m.y_movement(),
            MousePacket::Three(m) => m.generic_part().y_movement(),
            MousePacket::Four(m) => m.generic_part().y_movement(),
        }
    }
    pub fn scroll_movement(&self) -> i8 {
        match self {
            MousePacket::Three(m) => m.scroll_movement(),
            MousePacket::Four(m) => m.scroll_movement(),
            _ => 0,
        }
    }
    pub fn button_4(&self) -> bool {
        match self {
            MousePacket::Four(m) => m.button_4(),
            _ => false,
        }
    }
    pub fn button_5(&self) -> bool {
        match self {
            MousePacket::Four(m) => m.button_5(),
            _ => false,
        }
    }
}

#[derive(Clone)]
pub enum MouseSampleRate {
    _10 = 10,
    _20 = 20,
    _40 = 40,
    _60 = 60,
    _80 = 80,
    _100 = 100,
    _200 = 200
}

/// See [MousePacket] and its enum variants for further information.
#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum MouseId {
    /// the mouse has no scroll-movement
    Zero = 0,
    /// the mouse has scroll-movement
    Three = 3,
    /// the mouse has scroll-movement, fourth and fifth buttons
    Four = 4,
}

#[derive(Clone)]
pub enum MouseResolution {
    Count1PerMm = 0,
    Count2PerMm = 1,
    Count4PerMm = 2,
    Count8PerMm = 3
}

// NOTE: could be combined into a PS2DeviceType enum, see https://wiki.osdev.org/%228042%22_PS/2_Controller#Detecting_PS.2F2_Device_Types
pub enum KeyboardType {
    MF2Keyboard,
    MF2KeyboardWithPSControllerTranslator,
    AncientATKeyboard,
}
