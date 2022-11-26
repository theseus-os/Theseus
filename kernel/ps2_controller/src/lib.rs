#![no_std]

use log::{debug, warn};
use ps2::{write_command, flush_output_buffer, read_config, write_config, read_controller_test_result, read_port_test_result, PortTestResult, HostToControllerCommand::*, reset_keyboard, reset_mouse};
use fadt::Fadt;

// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
/// initialize the first and second (if it exists) PS/2 port
pub fn init() -> Result<(), &'static str> {
    // Step 1: Initialise USB Controllers
    // no USB support yet

    // Step 2: Determine if the PS/2 Controller Exists
    let acpi_tables = acpi::get_acpi_tables().lock();
    // if ACPI and therefore FADT is unsupported, PS/2 controller is assumed to exist
    if let Some(fadt) = Fadt::get(&acpi_tables) {
        // if earlier than ACPI v2, PS/2 controller is assumed to exist
        if fadt.header.revision > 1 {
            let has_controller = fadt.iapc_boot_architecture_flags & 0b10 == 0b10;
            if !has_controller {
                return Err("no PS/2 Controller present");
            }
        }
    }

    // Step 3: Disable Devices
    write_command(DisablePort1);
    write_command(DisablePort2);

    // Step 4: Flush The Output Buffer
    flush_output_buffer();

    // Step 5: Set the Controller Configuration Byte
    let mut config = read_config();
    config.set_port1_interrupt_enabled(false);
    config.set_port2_interrupt_enabled(false);
    config.set_port1_translation_enabled(false);
    let has_mouse = config.port2_clock_disabled();
    write_config(config);

    // Step 6: Perform Controller Self Test
    write_command(TestController);
    read_controller_test_result()?;
    debug!("passed PS/2 controller test");
    // as this can reset the controller on some hardware, we restore the config
    write_config(config);

    // Step 7: Determine If There Are 2 Channels
    // not needed, already done above

    // Step 8: Perform Interface Tests
    write_command(TestPort1);
    use PortTestResult::*;
    let port_1_result = match read_port_test_result()? {
        Passed => Ok("passed PS/2 port 1 test"),
        ClockLineStuckLow => Err("failed PS/2 port 1 test, clock line stuck low"),
        ClockLineStuckHigh => Err("failed PS/2 port 1 test, clock line stuck high"),
        DataLineStuckLow => Err("failed PS/2 port 1 test, data line stuck low"),
        DataLineStuckHigh => Err("failed PS/2 port 1 test, clock line stuck high"),
    };
    let port_2_result = if has_mouse {
        write_command(TestPort2);
        match read_port_test_result()? {
            Passed => Ok("passed PS/2 port 2 test"),
            ClockLineStuckLow => Err("failed PS/2 port 2 test, clock line stuck low"),
            ClockLineStuckHigh => Err("failed PS/2 port 2 test, clock line stuck high"),
            DataLineStuckLow => Err("failed PS/2 port 2 test, data line stuck low"),
            DataLineStuckHigh => Err("failed PS/2 port 2 test, clock line stuck high"),
        }
    } else {
        Err("PS/2 controller is not dual channel, so no mouse available")
    };
    if port_1_result.is_err() && port_2_result.is_err() {
        //TODO: does not need to crash the system if we have another input method (over USB)
        return Err("failed both PS/2 port tests, terminating PS/2 controller driver");
    }

    // Step 9: Enable Devices
    match port_1_result {
        Ok(_) => {
            write_command(EnablePort1);
            config.set_port1_interrupt_enabled(true);
        }
        Err(e) => warn!("{e}"),
    }
    match port_2_result {
        Ok(_) => {
            write_command(EnablePort2);
            config.set_port2_interrupt_enabled(true);
        }
        Err(e) => warn!("{e}"),
    }
    debug!("{config:?}");
    write_config(config);

    // Step 10: Reset Devices
    reset_keyboard()?;
    reset_mouse()?;
    Ok(())
}
