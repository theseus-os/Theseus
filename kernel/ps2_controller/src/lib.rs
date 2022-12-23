#![no_std]

use log::{debug, warn};
use spin::Once;
use fadt::Fadt;
use ps2::{PS2Controller, HostToControllerCommand::*};

// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
/// initialize the first and second (if it exists) PS/2 port
pub fn init() -> Result<&'static PS2Controller, &'static str> {
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

    let controller = unsafe { PS2Controller::new() };

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
    controller.keyboard_handle().reset()?;
    controller.mouse_handle().reset()?;

    debug!("Final PS/2 {:?}", controller.read_config());
    
    static CONTROLLER: Once<PS2Controller> = Once::new();
    Ok(CONTROLLER.call_once(|| controller))
}
