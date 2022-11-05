#![no_std]

use log::debug;
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

// https://wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_Commands
// quite a few of these are commented out because they're either unused, deprecated or non-standard.
/// Command types which can be sent to the PS/2 Controller
/// 
/// Naming perspective: Output means "out of the device into the host".
/// e.g. [WriteByteToPort2InputBuffer] means "write byte out of the host into port 2"
enum HostToControllerCommand {
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
    /// returns [ControllerTestResult]
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

/// Write a command to the PS/2 command port/register
/// 
/// Note: Devices attached to the controller should be disabled
/// before sending commands that return data, otherwise the output buffer could get overwritten
fn write_command(value: HostToControllerCommand) {
    unsafe {
        PS2_COMMAND_AND_STATUS_PORT.lock().write(value as u8);
    }
}

// workaround for `dead_code` warnings, https://github.com/Robbepop/modular-bitfield/issues/56
#[allow(dead_code)] fn hi() {ControllerToHostStatus::new().into_bytes();}
// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Status_Register
// and https://users.utcluj.ro/~baruch/sie/labor/PS2/PS-2_Keyboard_Interface.htm
#[bitfield(bits = 8)]
struct ControllerToHostStatus {
    /// When using [Polling](https://wiki.osdev.org/%228042%22_PS/2_Controller#Polling), must be `true` before attempting to read data from [PS2_DATA_PORT].
    /// When using [Interrupts](https://wiki.osdev.org/%228042%22_PS/2_Controller#Interrupts), guaranteed to be `true`, because an interrupt only happens if the buffer is full
    /// Alternative name: output_register_full, which might make more sense because it only ever fits 1 byte. "Buffer" sounds like multiple.
    output_buffer_full: bool,
    /// Must be `false` before attempting to write data to [PS2_DATA_PORT] or [PS2_COMMAND_AND_STATUS_PORT] for 8042 keyboard controller
    /// Alternative name: input_register_full, which might make more sense because it only ever fits 1 byte. "Buffer" sounds like multiple.
    input_buffer_full: bool,
    /// Cleared on reset; set when the system passed Power-on self-test
    #[allow(dead_code)]
    system_passed_self_test: bool,
    /// Input buffer should be written to: 0 - [PS2_DATA_PORT], 1 - [PS2_COMMAND_AND_STATUS_PORT]
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
    #[allow(dead_code)]
    mouse_output_buffer_full: bool,
    /// Timeout during keyboard command receive or response (Same as `transmit_timeout` + `receive_timeout`)
    #[allow(dead_code)]
    timeout_error: bool,
    /// Should be odd parity, set to `true` if even parity received
    #[allow(dead_code)]
    parity_error: bool,
}

/// Read the PS/2 status port/register
fn status_register() -> ControllerToHostStatus {
    ControllerToHostStatus::from_bytes([PS2_COMMAND_AND_STATUS_PORT.lock().read()])
}

/// Read data from the PS/2 data port.
/// 
/// Note: this can be called directly when within a PS/2 device's interrupt handler.
/// If polling is used (e.g. in a device's init function), [ControllerToHostStatus] `output_buffer_full` needs to be checked first.
fn read_data() -> u8 {
    PS2_DATA_PORT.lock().read()
}

/// All types of data writable to the [PS2_DATA_PORT].
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

/// Write data to the PS/2 data port.
/// 
/// Note: when writing to the PS/2 controller, it might be necessary to check for
/// [ControllerToHostStatus] `!input_buffer_full` first.
fn write_data(value: WritableData) {
    unsafe { PS2_DATA_PORT.lock().write(value.into()) };
}

// wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_Configuration_Byte
/// Used for [HostToControllerCommand::ReadFromInternalRAMByte0] and [HostToControllerCommand::WriteToInternalRAMByte0]
#[bitfield(bits = 8)]
#[derive(Debug)]
pub struct ControllerConfigurationByte {
    /// interrupt on [ControllerToHostStatus] `output_buffer_full`
    port1_interrupt_enabled: bool,
    /// interrupt on [ControllerToHostStatus] `mouse_output_buffer_full`
    /// 
    /// Note: only if 2 PS/2 ports supported
    port2_interrupt_enabled: bool,
    /// Cleared on reset; set when the system passed Power-on self-test
    #[allow(dead_code)]
    system_passed_self_test: bool,
    // or override_keyboard_inhibiting
    #[allow(dead_code)]
    should_be_zero: B1,
    /// disables the keyboard
    port1_clock_disabled: bool,
    /// disables the auxilary device (mouse)
    /// 
    /// Note: only if 2 PS/2 ports supported
    port2_clock_disabled: bool,
    /// whether IBM scancode translation is enabled (0=AT, 1=PC)
    #[allow(dead_code)]
    port1_translation_enabled: bool,
    #[allow(dead_code)]
    must_be_zero: B1,
}

/// read the config of the PS/2 port
fn read_config() -> ControllerConfigurationByte {
    write_command(ReadFromInternalRAMByte0);
    ControllerConfigurationByte::from_bytes([read_data()])
}

/// write the new config to the PS/2 command port (0x64)
fn write_config(value: ControllerConfigurationByte) {
    write_command(WriteToInternalRAMByte0);
    write_data(WritableData::Configuration(value));
}

/// initialize the first PS/2 data port
pub fn init_ps2_port1() {
    init_ps2_port(PS2Port::One);
}

/// initialize the second PS/2 data port
pub fn init_ps2_port2() {
    init_ps2_port(PS2Port::Two);
}
enum PS2Port {
    One,
    Two,
}

// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
fn init_ps2_port(port: PS2Port) {
    // Step 1: Initialise USB Controllers
    // no USB support yet

    // Step 2: Determine if the PS/2 Controller Exists
    // TODO: we support the FADT, so we can check this

    // Step 3: Disable Devices
    write_command(DisablePort2);
    write_command(DisablePort1);

    // Step 4: Flush The Output Buffer
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

    // Step 6/7/8 are inside [test_ps2_port]

    // Step 9: Enable Devices
    write_command(EnablePort1);
    match port {
        PS2Port::Two => write_command(EnablePort2),
        PS2Port::One => (),
    };
}

/// Clean the [PS2_DATA_PORT] output buffer, skipping the [ControllerToHostStatus] `output_buffer_full` check
fn flush_output_buffer() {
    //NOTE(hecatia): on my end, this is always 250 for port 1 and 65 for port 2, even if read multiple times
    debug!("ps2::flush_output_buffer: {}", read_data());
}

/// test the first PS/2 data port
pub fn test_ps2_port1() -> Result<(), &'static str> {
    test_ps2_port(PS2Port::One)
}

// test the second PS/2 data port
pub fn test_ps2_port2() -> Result<(), &'static str> {
    test_ps2_port(PS2Port::Two)
}

// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
fn test_ps2_port(port: PS2Port) -> Result<(), &'static str> {
    // Step 1-5 are inside [init_ps2_port]

    // Step 6: Perform Controller Self Test
    write_command(TestController);
    read_controller_test_result()?;
    debug!("passed PS/2 controller test");

    // TODO: Step 7, but not here, maybe init and test both devices at once in a new PS/2 controller driver crate

    // Step 8: Perform Interface Tests
    match port {
        PS2Port::One => write_command(TestPort1),
        PS2Port::Two => write_command(TestPort2),
    }
    use PortTestResult::*;
    match read_port_test_result()? {
        Passed => debug!("passed PS/2 port {} test", port as u8 + 1),
        ClockLineStuckLow => Err("failed PS/2 port test, clock line stuck low")?,
        ClockLineStuckHigh => Err("failed PS/2 port test, clock line stuck high")?,
        DataLineStuckLow => Err("failed PS/2 port test, data line stuck low")?,
        DataLineStuckHigh => Err("failed PS/2 port test, clock line stuck high")?,
    }

    // print the config (?) see TODO above
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
        debug!("PS/2 port {} interrupt enabled", port as u8 + 1)
    } else {
        Err("PS/2 port test config's interrupt disabled")?
    }
    if clock_enabled {
        debug!("PS/2 port {} clock enabled", port as u8 + 1)
    } else {
        Err("PS/2 port test config's clock disabled")?
    }
    Ok(())
}

/// must only be called after writing the [TestController] command
/// otherwise would read bogus data
fn read_controller_test_result() -> Result<(), &'static str> {
    const CONTROLLER_TEST_PASSED: u8 = 0x55;
    if read_data() != CONTROLLER_TEST_PASSED {
        Err("failed PS/2 controller test")
    } else {
        Ok(())
    }
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
    read_data().try_into().map_err(|_| "failed to read port test result")
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

// https://wiki.osdev.org/%228042%22_PS/2_Controller#Sending_Bytes_To_Device.2Fs
// and https://wiki.osdev.org/PS/2_Mouse#Set_Sample_Rate_Example
// AND https://wiki.osdev.org/%228042%22_PS/2_Controller#Polling, which still has some _maybe relevant information_:
// We might still need to disable one of the devices every time we send so we can read_data reliably.
// As it currently works, it might only be a problem for older devices, so I won't overoptimize.
/// write data to the data port and return the response
fn send(value: HostToDevice) -> Result<DeviceToHostResponse, &'static str> {
    const VERY_ARBITRARY_TIMEOUT_VALUE: u8 = 3;
    for _ in 0..VERY_ARBITRARY_TIMEOUT_VALUE {
        // so that we don't overwrite the previously sent command
        if !status_register().input_buffer_full() {
            write_data(WritableData::HostToDevice(value.clone()));
            // so that we only read when data is available
            if status_register().output_buffer_full() {
                return read_data().try_into().map_err(|_| "failed to read device response");
            }
        }
    }
    Err("timeout value exceeded")
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
    GetCurrentSet = 0,
    Set1 = 1,
    Set2 = 2,
    Set3 = 3,
}

/// write command to the keyboard and return the result
fn command_to_keyboard(value: HostToKeyboardCommandOrData) -> Result<(), &'static str> {
    for _ in 0..3 {
        let response = send(HostToDevice::Keyboard(value.clone()));

        match response {
            Ok(Acknowledge) => return Ok(()),
            Ok(ResendCommand) => continue,
            _ => continue,
        }
    }
    Err("keyboard doesn't support the command or there has been a hardware failure")
}

#[derive(Clone)]
pub enum HostToMouseCommandOrData {
    MouseCommand(HostToMouseCommand),
    SampleRate(SampleRate),
    MouseResolution(MouseResolution),
}

// https://wiki.osdev.org/PS/2_Mouse#Mouse_Device_Over_PS.2F2
#[derive(Clone)]
pub enum HostToMouseCommand {
    SetScaling = 0xE6, //1 or 2
    SetResolution = 0xE8, //[MouseResolution]
    StatusRequest = 0xE9,
    SetStreamMode = 0xEA,
    ReadData = 0xEB,
    ResetWrapMode = 0xEC,
    SetWrapMode = 0xEE,
    SetRemoteMode = 0xF0, //NOTE: same value as SetScancodeSet
    GetDeviceID = 0xF2, //NOTE: same value as IdentifyKeyboard
    SampleRate = 0xF3,  //NOTE: same value as SetRepeatRateAndDelay
    EnableDataReporting = 0xF4, //NOTE: same value as EnableScanning
    DisableDataReporting = 0xF5, //NOTE: same value as DisableScanning
    SetDefaults = 0xF6, //NOTE: same
    ResendByte = 0xFE,  //NOTE: same
    Reset = 0xFF,       //NOTE: same
}

/// write command to the mouse and return the result
fn command_to_mouse(value: HostToMouseCommandOrData) -> Result<(), &'static str> {
    for _ in 0..3 {
        write_command(WriteByteToPort2InputBuffer);
        let response = send(HostToDevice::Mouse(value.clone()));

        match response {
            Ok(Acknowledge) => return Ok(()),
            _ => continue,
        }
    }
    Err("mouse doesn't support the command or there has been a hardware failure")
}

//TODO: different MousePacketBits for different MouseId?
// see https://wiki.osdev.org/PS/2_Mouse#5_buttons
#[bitfield(bits = 32)]
#[derive(Debug)]
pub struct MousePacketBits4 {
    //1. byte starts here
    pub button_left: bool,
    pub button_right: bool,
    pub button_middle: bool,
    pub always_one: B1,
    /// see [x_1st_to_8th_bit]
    x_9th_bit: bool,
    /// see [y_1st_to_8th_bit]
    y_9th_bit: bool,
    pub x_overflow: bool,
    pub y_overflow: bool,
    //2. byte
    /// only a part of x_movement, needs to be combined with [x_9th_bit]
    x_1st_to_8th_bit: B8, //u8 with #[bits = 8] attribute didn't work
    //3. byte
    /// only a part of y_movement, needs to be combined with [y_9th_bit]
    y_1st_to_8th_bit: B8, //u8 with #[bits = 8] attribute didn't work
    //4. byte starts here
    /// already stored in two's complement
    z_movement: B4, //u8 with #[bits = 4] attribute didn't work
    pub button_4: bool,
    pub button_5: bool,
    #[allow(dead_code)]
    zero1: B1,
    #[allow(dead_code)]
    zero2: B1,
}

impl MousePacketBits4 {
    /// `x_1st_to_8th_bit` and `x_9th_bit` should not be accessed directly, because they're part of one signed 9-bit number
    pub fn x_movement(&self) -> i16 {
        Self::to_2s_complement(self.x_1st_to_8th_bit(), self.x_9th_bit())
    }

    /// `y_1st_to_8th_bit` and `y_9th_bit` should not be accessed directly, because they're part of one signed 9-bit number
    pub fn y_movement(&self) -> i16 {
        Self::to_2s_complement(self.y_1st_to_8th_bit(), self.y_9th_bit())
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

    //currently limited to mouse id 4
    /// often called `z_movement`, renamed to disambiguate
    pub fn scroll_movement(&self) -> i8 {
        self.z_movement() as i8 //TODO: see osdev wiki, might be wrong
    }
}

/// read mouse data packet; will work for mouse with ID 4 and probably 3
pub fn read_mouse_packet() -> MousePacketBits4 {
    MousePacketBits4::from_bytes([
        read_data(),
        read_data(),
        read_data(),
        read_data()
    ])
}

#[derive(Clone)]
pub enum SampleRate {
    _10 = 10,
    _20 = 20,
    _40 = 40,
    _60 = 60,
    _80 = 80,
    _100 = 100,
    _200 = 200
}

/// set PS/2 mouse's sampling rate
fn set_mouse_sampling_rate(value: SampleRate) -> Result<(), &'static str> {
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(SampleRate))
        .and_then(|_| command_to_mouse(HostToMouseCommandOrData::SampleRate(value)))
        .map_err(|_| "failed to set the mouse sampling rate")
}

pub enum MouseId {
    /// the mouse has scroll-movement
    Three,
    /// the mouse has scroll-movement, fourth and fifth buttons
    Four,
}

/// set the [MouseId] by magic sequence
pub fn set_mouse_id(id: MouseId) -> Result<(), &'static str> {
    disable_mouse_packet_streaming()?;

    use crate::SampleRate::*;
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

    enable_mouse_packet_streaming()?;
    Ok(())
}


pub fn mouse_id() -> Result<u8, &'static str> {
    disable_mouse_packet_streaming()?;

    // check whether the command is acknowledged
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(GetDeviceID))?;
    let id_num = read_data();

    enable_mouse_packet_streaming()?;
    Ok(id_num)
}

/// reset the mouse
#[allow(dead_code)]
fn reset_mouse() -> Result<(), &'static str> {
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(Reset))?; 
    for _ in 0..14 {
        //BAT = Basic Assurance Test
        let bat_code = read_data();
        if bat_code == 0xAA {
            // command reset mouse succeeded
            return Ok(());
        }
    }
    Err("failed to reset mouse")
}

/// resend the most recent packet again
#[allow(dead_code)]
fn mouse_resend() -> Result<(), &'static str> {
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(HostToMouseCommand::ResendByte))
        .map_err(|_| "failed to resend mouse request")
}

/// enable the packet streaming
fn enable_mouse_packet_streaming() -> Result<(), &'static str> {
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(EnableDataReporting)).map_err(|_| {
        "failed to enable mouse streaming"
    })
}

/// disable the packet streaming
fn disable_mouse_packet_streaming() -> Result<(), &'static str> {
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(DisableDataReporting)).map_err(|_| {
        "failed to disable mouse streaming"
    })
}

#[derive(Clone)]
pub enum MouseResolution {
    Count1PerMm = 0,
    Count2PerMm = 1,
    Count4PerMm = 2,
    Count8PerMm = 3
}

/// set the resolution of the mouse
#[allow(dead_code)]
fn mouse_resolution(value: MouseResolution) -> Result<(), &'static str> {
    command_to_mouse(HostToMouseCommandOrData::MouseCommand(SetResolution))
        .and_then(|_| command_to_mouse(HostToMouseCommandOrData::MouseResolution(value)))
        .map_err(|_| "failed to set the mouse resolution")
}

/// set LED status of the keyboard
pub fn set_keyboard_led(value: LEDState) -> Result<(), &'static str> {
    command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(SetLEDStatus))
        .and_then(|_| command_to_keyboard(HostToKeyboardCommandOrData::LEDState(value)))
        .map_err(|_| "failed to set the keyboard led")
}

/// set the scancode set of the keyboard
pub fn keyboard_scancode_set(value: ScancodeSet) -> Result<(), &'static str> {
    command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(SetScancodeSet))
        .and_then(|_| command_to_keyboard(HostToKeyboardCommandOrData::ScancodeSet(value)))
        .map_err(|_| "failed to set the keyboard scancode set")
}

//NOTE: could be combined into a PS2DeviceType enum, see https://wiki.osdev.org/%228042%22_PS/2_Controller#Detecting_PS.2F2_Device_Types
pub enum KeyboardType {
    MF2Keyboard,
    MF2KeyboardWithPSControllerTranslator,
    AncientATKeyboard,
}

///detect the keyboard's type
pub fn keyboard_detect() -> Result<KeyboardType, &'static str> {
    command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(DisableScanning))?;

    command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(IdentifyKeyboard))?; 

    let keyboard_type = match read_data() {
        //None => Ok(KeyboardType::AncientATKeyboard)
        0xAB => {
            match read_data() {
                0x41 | 0xC1 => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
                0x83 => Ok(KeyboardType::MF2Keyboard),
                _ => Err("unrecognized keyboard type")
            }
        }
        _ => Err("unrecognized keyboard type")
    };
    command_to_keyboard(HostToKeyboardCommandOrData::KeyboardCommand(EnableScanning))?;
    keyboard_type
}

pub fn read_scancode() -> u8 {
    read_data()
}
