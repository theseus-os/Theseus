#![no_std]

use log::{warn, info, trace};
use num_enum::TryFromPrimitive;
use port_io::Port;
use spin::Mutex;
use modular_bitfield::{specifiers::{B1, B4, B8}, bitfield};

use HostToControllerCommand::*;
use HostToKeyboardCommand::*;
use HostToMouseCommand::*;
use DeviceToHostResponse::*;

/// Port used by PS/2 Controller and devices
static PS2_DATA_PORT: Mutex<Port<u8>> = Mutex::new(Port::new(0x60));
/// Port used to send commands to and receive status from the PS/2 Controller
static PS2_COMMAND_AND_STATUS_PORT: Mutex<Port<u8>> = Mutex::new(Port::new(0x64));

/// Clean the [PS2_DATA_PORT] output buffer, skipping the [ControllerToHostStatus] `output_buffer_full` check
fn flush_output_buffer() {
    //NOTE(hecatia): on my end, this is always 250 for port 1 and 65 for port 2, even if read multiple times
    trace!("{}", read_data());
}

// https://wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_Commands
/// Command types which can be sent to the PS/2 Controller
enum HostToControllerCommand {
    /// returns [ControllerConfigurationByte]
    ReadFromInternalRAMByte0 = 0x20,
    // ReadFromInternalRAMByteN = 0x21, //0x21-0x3F; N is the command byte & 0x1F; return Unknown/non-standard
    /// sets [ControllerConfigurationByte]
    WriteToInternalRAMByte0 = 0x60,
    // WriteToInternalRAMByteN = 0x61, //0x61-0x7F; N is the command byte & 0x1F
    DisablePort2 = 0xA7, //NOTE: only if 2 PS/2 ports supported
    EnablePort2 = 0xA8,  //NOTE: only if 2 PS/2 ports supported
    /// returns [TestPortResult]
    TestPort2 = 0xA9, //NOTE: only if 2 PS/2 ports supported
    /// returns [TestControllerResult]
    TestController = 0xAA,
    /// returns [TestPortResult]
    TestPort1 = 0xAB,
    ///// read all bytes of internal RAM
    // DiagnosticDump = 0xAC,
    DisablePort1 = 0xAD,
    EnablePort1 = 0xAE,
    // ReadControllerInputPort = 0xC0, //return Unknown/non-standard
    // CopyBits0to3ofInputPortToStatusBits4to7 = 0xC1,
    // CopyBits4to7ofInputPortToStatusBits4to7 = 0xC2,
    ///// returns [ControllerOutputPort] (not implemented, https://wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_Output_Port)
    // ReadControllerOutputPort = 0xD0,
    ///// reads [ControllerOutputPort] (not implemented, https://wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_Output_Port)
    // WriteByteToControllerOutputPort = 0xD1, //Check if output buffer is empty first
    ///// makes it look like the byte written was received from the first PS/2 port
    // WriteByteToPort1OutputBuffer = 0xD2, //NOTE: only if 2 PS/2 ports supported
    /// makes it look like the byte written was received from the second PS/2 port
    WriteByteToPort2OutputBuffer = 0xD3, //NOTE: only if 2 PS/2 ports supported //https://wiki.osdev.org/%228042%22_PS/2_Controller#Buffer_Naming_Perspective OUTPUT means "out of the device into host"
    /// sends next byte to the second PS/2 port
    WriteByteToPort2InputBuffer = 0xD4, //NOTE: only if 2 PS/2 ports supported
    // PulseOutputLineLowFor6ms = 0xF0, //0xF0-0xFF; Bits 0 to 3 correspond to 4 different output lines and are used as a mask (0 = pulse line, 1 = don't pulse line); Bit 0 corresponds to the "reset" line. The other output lines don't have a standard/defined purpose.
}

/// Write a command to the PS/2 command port/register
fn write_command(value: HostToControllerCommand) {
    unsafe {
        PS2_COMMAND_AND_STATUS_PORT.lock().write(value as u8);
    }
}

#[allow(non_camel_case_types)]
type u1 = B1;
#[allow(non_camel_case_types)]
type bool1 = bool;

// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Status_Register
// and https://users.utcluj.ro/~baruch/sie/labor/PS2/PS-2_Keyboard_Interface.htm
// `pub` because of `dead_code` warnings, see https://github.com/Robbepop/modular-bitfield/issues/56
#[bitfield(bits = 8)]
pub struct ControllerToHostStatus {
    /// When using [Polling](https://wiki.osdev.org/%228042%22_PS/2_Controller#Polling), must be `true` before attempting to read data from [PS2_DATA_PORT].
    /// When using [Interrupts](https://wiki.osdev.org/%228042%22_PS/2_Controller#Interrupts), guaranteed to be `true`, because an interrupt only happens if the buffer is full
    output_buffer_full: bool1,
    /// Must be `false` before attempting to write data to [PS2_DATA_PORT] or [PS2_COMMAND_AND_STATUS_PORT]
    input_buffer_full: bool1,
    /// Cleared on reset; set when the system passed Power-on self-test
    system_passed_self_test: bool1,
    /// Input buffer should be written to: 0 - [PS2_DATA_PORT], 1 - [PS2_COMMAND_AND_STATUS_PORT]
    input_buffer_is_command: u1,
    /// Whether or not communication is inhibited (via switch) / Unknown (chipset specific) / (more likely unused on modern systems)
    keyboard_enabled: bool1,
    ///// Keyboard didn't generate clock signals within 15 ms of "request-to-send" (exclusive to AT-compatible mode)
    // transmit_timeout: bool1,
    ///// Keyboard didn't generate clock signals within 20 ms of command reception (exclusive to AT-compatible mode)
    // receive_timeout: bool1,
    /// Similar to `output_buffer_full`, except for mouse
    mouse_output_buffer_full: bool1,
    /// Timeout during keyboard command receive or response (Same as [transmit_timeout] + [receive_timeout])
    timeout_error: bool1,
    /// Should be odd parity, set to `true` if even parity received
    parity_error: bool1,
}

/// Read the PS/2 status port/register
/// Note: Currently unused, might be used later for error checking
fn status_register() -> ControllerToHostStatus {
    ControllerToHostStatus::from_bytes([PS2_COMMAND_AND_STATUS_PORT.lock().read()])
}

/// Read data from the PS/2 data port
fn read_data() -> u8 {
    PS2_DATA_PORT.lock().read()
}

/// Write data to the PS/2 data port
fn write_data(value: u8) {
    unsafe { PS2_DATA_PORT.lock().write(value) };
}

// wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_Configuration_Byte
/// Used for [HostToControllerCommand::ReadFromInternalRAMByte0] and [HostToControllerCommand::WriteToInternalRAMByte0]
#[bitfield(bits = 8)]
#[derive(Debug)]
pub struct ControllerConfigurationByte {
    port1_interrupt_enabled: bool1,
    port2_interrupt_enabled: bool1, //NOTE: only if 2 PS/2 ports supported
    /// Cleared on reset; set when the system passed Power-on self-test
    system_passed_self_test: bool1,
    should_be_zero: u1,
    port1_clock_disabled: bool1,
    port2_clock_disabled: bool1, //NOTE: only if 2 PS/2 ports supported
    port1_translation: bool1,
    must_be_zero: u1,
}

/// read the config of the ps2 port
fn read_config() -> ControllerConfigurationByte {
    write_command(ReadFromInternalRAMByte0);
    ControllerConfigurationByte::from_bytes([read_data()])
}

/// write the new config to the ps2 command port (0x64)
fn write_config(value: ControllerConfigurationByte) {
    write_command(WriteToInternalRAMByte0);
    write_data(value.into_bytes()[0]);
}

/// initialize the first ps2 data port
pub fn init_ps2_port1() {
    init_ps2_port(PS2Port::One);
}

/// initialize the second ps2 data port
pub fn init_ps2_port2() {
    init_ps2_port(PS2Port::Two);
}
enum PS2Port {
    One,
    Two,
}

// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
fn init_ps2_port(port: PS2Port) {
    // Step 3: Disable Devices
    write_command(DisablePort2);
    write_command(DisablePort1);

    // Step 4
    flush_output_buffer();

    // Step 5: Set the Controller Configuration Byte
    let mut config = read_config();
    match port {
        PS2Port::One => {
            config.set_port1_clock_disabled(false);
            config.set_port1_interrupt_enabled(true);
        }
        PS2Port::Two => {
            config.set_port2_clock_disabled(false);
            config.set_port2_interrupt_enabled(true);
        }
    }
    write_config(config);

    // Step 9: Enable Devices
    write_command(EnablePort1);
    match port {
        PS2Port::Two => write_command(EnablePort2),
        PS2Port::One => (),
    };
}

/// test the first ps2 data port
pub fn test_ps2_port1() -> Result<(), &'static str> {
    test_ps2_port(PS2Port::One)
}

// test the second ps2 data port
pub fn test_ps2_port2() -> Result<(), &'static str> {
    test_ps2_port(PS2Port::Two)
}

// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
fn test_ps2_port(port: PS2Port) -> Result<(), &'static str> {
    // test the ps2 controller
    write_command(TestController);
    match read_controller_test_result()? {
        ControllerTestResult::Passed => info!("ps2 controller test pass!!!"),
        ControllerTestResult::Failed => warn!("ps2 controller test failed!!!"),
    }

    // test the ps2 data port
    match port {
        PS2Port::One => write_command(TestPort1),
        PS2Port::Two => write_command(TestPort2),
    }
    use PortTestResult::*;
    match read_port_test_result()? {
        Passed => info!("ps2 port {} test pass!!!", port as u8 + 1),
        ClockLineStuckLow => warn!("ps2 port {} clock line stuck low", port as u8 + 1),
        ClockLineStuckHigh => warn!("ps2 port {} clock line stuck high", port as u8 + 1),
        DataLineStuckLow => warn!("ps2 port {} data line stuck low", port as u8 + 1),
        DataLineStuckHigh => warn!("ps2 port {} data line stuck high", port as u8 + 1),
    }

    // enable PS2 interrupt and see the new config
    let config = read_config();
    let port_interrupt_enabled = match port {
        PS2Port::One => config.port1_interrupt_enabled(),
        PS2Port::Two => config.port2_interrupt_enabled(),
    };
    let clock_enabled = match port {
        PS2Port::One => !config.port1_clock_disabled(),
        PS2Port::Two => !config.port2_clock_disabled(),
    };

    if port_interrupt_enabled {
        info!("ps2 port {} interrupt enabled", port as u8 + 1)
    } else {
        warn!("ps2 port {} interrupt disabled", port as u8 + 1)
    }
    if clock_enabled {
        info!("ps2 port {} enabled", port as u8 + 1)
    } else {
        warn!("ps2 port {} disabled", port as u8 + 1)
    }
    Ok(())
}

#[derive(TryFromPrimitive)]
#[repr(u8)]
enum ControllerTestResult {
    Passed = 0x55,
    Failed = 0xFC,
}

/// must only be called after writing the [TestController] command
/// otherwise would read bogus data
fn read_controller_test_result() -> Result<ControllerTestResult, &'static str> {
    read_data().try_into().map_err(|_| "read_controller_test_result called at the wrong time")
}

#[derive(TryFromPrimitive)]
#[repr(u8)]
enum PortTestResult {
    Passed = 0x00,
    ClockLineStuckLow = 0x01,
    ClockLineStuckHigh = 0x02,
    DataLineStuckLow = 0x03,
    DataLineStuckHigh = 0x04,
}

/// must only be called after writing the [TestPort1] or [TestPort2] command
/// otherwise would read bogus data
fn read_port_test_result() -> Result<PortTestResult, &'static str> {
    read_data().try_into().map_err(|_| "read_port_test_result called at the wrong time")
}

// https://wiki.osdev.org/PS/2_Keyboard#Special_Bytes //NOTE: all other bytes sent by the keyboard are scan codes
// http://users.utcluj.ro/~baruch/sie/labor/PS2/PS-2_Mouse_Interface.htm mouse only returns AA, FC or FA, mouse_id and packet
#[derive(TryFromPrimitive)]
#[repr(u8)]
pub enum DeviceToHostResponse {
    KeyDetectionErrorOrInternalBufferOverrun1 = 0x00,
    SelfTestPassed = 0xAA, //sent after "0xFF (reset)" command or keyboard power up
    ResponseToEcho = 0xEE,
    Acknowledge = 0xFA,
    SelfTestFailed1 = 0xFC, //sent after "0xFF (reset)" command or keyboard power up
    SelfTestFailed2 = 0xFD, //sent after "0xFF (reset)" command or keyboard power up
    ResendCommand = 0xFE,
    KeyDetectionErrorOrInternalBufferOverrun2 = 0xFF,
}

/// write data to the first ps2 data port and return the response
fn data_to_port1(value: u8) -> DeviceToHostResponse {
    write_data(value);
    read_data().try_into().map_err(|e| warn!("{:?}", e)).unwrap() //FIXME: for testing
}

/// write data to the second ps2 data port and return the response //TODO: MouseResponse
fn data_to_port2(value: u8) -> DeviceToHostResponse {
    write_command(WriteByteToPort2InputBuffer);
    data_to_port1(value)
}

// https://wiki.osdev.org/PS/2_Keyboard#Commands
pub enum HostToKeyboardCommandOrData {
    KeyboardCommand(HostToKeyboardCommand),
    LEDState(LEDState),
    ScancodeSet(ScancodeSet),
    //TODO: Typematic, Scancode
}

pub enum HostToKeyboardCommand {
    SetLEDStatus = 0xED,
    ///// for diagnostic purposes and useful for device removal detection
    //Echo = 0xEE,
    SetScancodeSet = 0xF0,
    IdentifyKeyboard = 0xF2,
    /// also called typematic
    SetRepeatRateAndDelay = 0xF3,
    EnableScanning = 0xF4,
    /// might also restore default parameters
    DisableScanning = 0xF5,
    //SetDefaultParameters = 0xF6,
    ///// scancode set 3 only
    //SetAllKeysToAutorepeat = 0xF7,
    ///// scancode set 3 only
    //SetAllKeysToMakeRelease = 0xF8,
    ///// scancode set 3 only
    //SetAllKeysToMake = 0xF9,
    ///// scancode set 3 only
    //SetAllKeysToAutorepeatMakeRelease = 0xFA, //NOTE: same value as ACK
    ///// scancode set 3 only
    //SetKeyToAutorepeat = 0xFB,
    ///// scancode set 3 only
    //SetKeyToMakeRelease = 0xFC, //NOTE: same value as Self test failed
    ///// scancode set 3 only
    //SetKeyToMake = 0xFD,
    ResendByte = 0xFE,
    ResetAndStartSelfTest = 0xFF,
}

#[bitfield(bits = 3)]
pub struct LEDState {
    pub scroll_lock: bool,
    pub number_lock: bool,
    pub caps_lock: bool,
}

pub enum ScancodeSet {
    GetCurrentSet = 0,
    Set1 = 1,
    Set2 = 2,
    Set3 = 3,
}

/// write command to the keyboard and return the result
fn command_to_keyboard(value: HostToKeyboardCommandOrData) -> Result<(), &'static str> {
    let response = match value {
        HostToKeyboardCommandOrData::KeyboardCommand(c) => data_to_port1(c as u8),
        HostToKeyboardCommandOrData::LEDState(l) => data_to_port1(l.into_bytes()[0]),
        HostToKeyboardCommandOrData::ScancodeSet(s) => data_to_port1(s as u8),
    };

    match response {
        Acknowledge => Ok(()),
        ResendCommand => Err("Fail to send the command to the keyboard"),
        _ => {
            for _x in 0..14 {
                // wait for response
                let response = read_data().try_into();
                match response {
                    Ok(Acknowledge) => return Ok(()),
                    Ok(ResendCommand) => return Err("Please resend the command to the keyboard"),
                    _ => (),
                }
            }
            Err("command is not acknowledged")
        }
    }
}

pub enum HostToMouseCommandOrData {
    MouseCommand(HostToMouseCommand),
    SampleRate(SampleRate),
    MouseResolution(MouseResolution),
}


// https://wiki.osdev.org/PS/2_Mouse#Mouse_Device_Over_PS.2F2
pub enum HostToMouseCommand {
    SetScaling = 0xE6, //1 or 2
    SetResolution = 0xE8, //[MouseResolution]
    StatusRequest = 0xE9,
    SetStreamMode = 0xEA,
    ReadData = 0xEB,
    ResetWrapMode = 0xEC,
    SetWrapMode = 0xEE,
    SetRemoteMode = 0xF0, //NOTE: same value as SetScancodeSet
    GetDeviceID = 0xF2, //NOTE: same value as IdentifyKeyboard; TODO: See Detecting PS/2 Device Types for the response bytes
    SampleRate = 0xF3,  //NOTE: same value as SetRepeatRateAndDelay; valid values are 10, 20, 40, 60, 80, 100, 200
    EnableDataReporting = 0xF4, //NOTE: same value as EnableScanning
    DisableDataReporting = 0xF5, //NOTE: same value as DisableScanning
    SetDefaults = 0xF6, //NOTE: same
    ResendByte = 0xFE,  //NOTE: same
    Reset = 0xFF,       //NOTE: same
}

/// write command to the mouse and return the result
fn command_to_mouse(value: HostToMouseCommandOrData) -> Result<(), &'static str> {
    let response = match value {
        HostToMouseCommandOrData::MouseCommand(c) => data_to_port2(c as u8),
        HostToMouseCommandOrData::MouseResolution(r) => data_to_port2(r as u8),
        HostToMouseCommandOrData::SampleRate(s) => data_to_port2(s as u8),
    };

    match response {
        Acknowledge => Ok(()),
        _ => {
            for _x in 0..14 {
                // wait for response
                let response = read_data().try_into();
                #[allow(clippy::single_match)]
                match response {
                    Ok(Acknowledge) => return Ok(()),
                    _ => (), //no Ok(Resend) check for mouse
                }
            }
            warn!("mouse command to second port is not accepted");
            Err("mouse command is not responded!!!")
        }
    }
}

/// write data to the second ps2 output buffer
fn write_to_second_output_buffer(value: u8) {
    write_command(WriteByteToPort2OutputBuffer);
    write_data(value);
}

//TODO: different MousePacketBits for different MouseId?
// see https://wiki.osdev.org/PS/2_Mouse#5_buttons
#[bitfield(bits = 32)]
#[derive(Debug)]
pub struct MousePacketBits4 {
    //1. byte starts here
    pub button_left: bool1,
    pub button_right: bool1,
    pub button_middle: bool1,
    pub always_one: u1,
    /// see [x_1st_to_8th_bit]
    x_9th_bit: bool1,
    /// see [y_1st_to_8th_bit]
    y_9th_bit: bool1,
    pub x_overflow: bool1,
    pub y_overflow: bool1,
    //2. byte
    /// only a part of x_movement, needs to be combined with [x_9th_bit]
    x_1st_to_8th_bit: B8, //u8 with #[bits = 8] attribute didn't work
    //3. byte
    /// only a part of y_movement, needs to be combined with [y_9th_bit]
    y_1st_to_8th_bit: B8, //u8 with #[bits = 8] attribute didn't work
    //4. byte starts here
    /// already stored in two's complement
    z_movement: B4, //u8 with #[bits = 4] attribute didn't work
    pub button_4: bool1,
    pub button_5: bool1,
    zero1: u1,
    zero2: u1,
}

impl MousePacketBits4 {
    /// `x_1st_to_8th_bit` and `x_9th_bit` should not be accessed directly, because they're part of one signed 9-bit number
    pub fn x_movement(&self) -> i16 {
        Self::combine_and_2s_complement(self.x_1st_to_8th_bit(), self.x_9th_bit())
    }

    /// `y_1st_to_8th_bit` and `y_9th_bit` should not be accessed directly, because they're part of one signed 9-bit number
    pub fn y_movement(&self) -> i16 {
        Self::combine_and_2s_complement(self.y_1st_to_8th_bit(), self.y_9th_bit())
    }

    // implementation from https://wiki.osdev.org/PS/2_Mouse
    fn combine_and_2s_complement(bit1to8: u8, bit9: bool) -> i16 {
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

    //currently limited to mouse id 4
    /// often called `z_movement`, renamed to disambiguate
    pub fn scroll_movement(&self) -> i8 {
        self.z_movement() as i8 //TODO: see osdev wiki, might be wrong
    }
}

/// read mouse data packet; will work for mouse with ID 4 and probably 3
pub fn read_mouse_packet() -> MousePacketBits4 {
    let packet = MousePacketBits4::from_bytes([
        read_data(),
        read_data(),
        read_data(),
        read_data()
    ]);
    info!("read_mouse_packet: {packet:?}");
    packet
}

pub enum SampleRate {
    _10 = 10,
    _20 = 20,
    _40 = 40,
    _60 = 60,
    _80 = 80,
    _100 = 100,
    _200 = 200
}

/// set ps2 mouse's sampling rate
fn set_mouse_sampling_rate(value: SampleRate) -> Result<(), &'static str> {
    // if command is not acknowledged
    if let Err(_e) = command_to_mouse(HostToMouseCommandOrData::MouseCommand(SampleRate)) {
        Err("set mouse sampling rate failled, please try again")
    } else {
        // if second byte command is not acknowledged
        if let Err(_e) = command_to_mouse(HostToMouseCommandOrData::SampleRate(value)) {
            Err("set mouse sampling rate failled, please try again")
        } else {
            Ok(())
        }
    }
}

pub enum MouseId {
    /// the mouse has scroll-movement
    Three,
    /// the mouse has scroll-movement, fourth and fifth buttons
    Four,
}

/// set the [MouseId] by magic sequence
pub fn set_mouse_id(id: MouseId) -> Result<(), &'static str> {
    disable_mouse_packet_streaming().map_err(|_| "failed to disable mouse streaming")?;

    use SampleRate::*;
    match id {
        MouseId::Three => {
            for rate in [_200, _100, _80] {
                set_mouse_sampling_rate(rate)?;
            }
        }
        //(maybe) TODO: apparently this needs to check after the first 80 if mouse_id is 3
        MouseId::Four => {
            for rate in [_200, _100, _80, _200, _200, _80] {
                set_mouse_sampling_rate(rate)?;
            }
        }
    }

    enable_mouse_packet_streaming().map_err(|_| "failed to enable mouse streaming after set_mouse_id")?;
    Ok(())
}


pub fn mouse_id() -> Result<u8, &'static str> {
    disable_mouse_packet_streaming().map_err(|_| "failed to disable mouse streaming")?;

    // check whether the command is acknowledged
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(GetDeviceID))?;
    let id_num = read_data();

    enable_mouse_packet_streaming().map_err(|_| "failed to enable mouse streaming after mouse_id")?;
    Ok(id_num)
}

/// reset the mouse
fn reset_mouse() -> Result<(), &'static str> {
    if let Err(_e) = command_to_mouse(HostToMouseCommandOrData::MouseCommand(Reset)) {
        Err("reset mouse failled please try again")
    } else {
        for _x in 0..14 {
            let more_bytes = read_data();
            if more_bytes == 0xAA {
                // command reset mouse succeeded
                return Ok(());
            }
        }
        warn!("disable streaming failed");
        Err("fail to command reset mouse fail!!!")
    }
}

/// resend the most recent packet again
fn mouse_resend() -> Result<(), &'static str> {
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(HostToMouseCommand::ResendByte))
        .map_err(|_| "mouse resend request failed, please request again")
}

/// enable the packet streaming
fn enable_mouse_packet_streaming() -> Result<(), &'static str> {
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(EnableDataReporting)).map_err(|_| {
        warn!("enable streaming failed");
        "enable mouse streaming failed"
    })
}

/// disable the packet streaming
fn disable_mouse_packet_streaming() -> Result<(), &'static str> {
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(DisableDataReporting)).map_err(|_| {
        warn!("disable mouse streaming failed");
        "disable mouse streaming failed"
    })
}

pub enum MouseResolution {
    Count1PerMm = 0,
    Count2PerMm = 1,
    Count4PerMm = 2,
    Count8PerMm = 3
}

/// set the resolution of the mouse
fn mouse_resolution(value: MouseResolution) -> Result<(), &'static str> {
    if let Err(_e) = command_to_mouse(HostToMouseCommandOrData::MouseCommand(SetResolution)) {
        warn!("command set mouse resolution is not accepted");
        Err("set mouse resolution failed!!!")
    } else if let Err(_e) = command_to_mouse(HostToMouseCommandOrData::MouseResolution(value)) {
        warn!("the resolution value is not accepted");
        Err("set mouse resolution failed!!!")
    } else {
        Ok(())
    }
}

/// set LED status of the keyboard
pub fn keyboard_led(value: LEDState) {
    if let Err(_e) = command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(SetLEDStatus)) {
        warn!("failed to set the keyboard led");
    } else if let Err(_e) = command_to_keyboard(HostToKeyboardCommandOrData::LEDState(value)) {
        warn!("failed to set the keyboard led");
    }
}

/// set the scancode set of the keyboard
fn keyboard_scancode_set(value: ScancodeSet) -> Result<(), &'static str> {
    if let Err(_e) = command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(SetScancodeSet)) {
        return Err("failed to set the keyboard scancode set");
    } else if let Err(_e) = command_to_keyboard(HostToKeyboardCommandOrData::ScancodeSet(value)) {
        return Err("failed to set the keyboard scancode set");
    }
    Ok(())
}

pub enum KeyboardType {
    MF2Keyboard,
    MF2KeyboardWithPSControllerTranslator,
    AncientATKeyboard,
}

///detect the keyboard's type
pub fn keyboard_detect() -> Result<KeyboardType, &'static str> {
    command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(IdentifyKeyboard))?; 
    let reply = read_data();
    match reply {
        0xAB => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
        0x41 => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
        0xC1 => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
        0x83 => Ok(KeyboardType::MF2Keyboard),
        _ => Ok(KeyboardType::AncientATKeyboard),
    }
}

pub fn read_scancode() -> u8 {
    read_data()
}
