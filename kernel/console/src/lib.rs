//! Creation and management of virtual consoles or terminals atop Theseus.

#![no_std]

extern crate alloc;

use alloc::{format, sync::Arc};
use async_channel::Receiver;
use core::sync::atomic::{AtomicU16, Ordering};
use core2::io::Write;
use irq_safety::MutexIrqSafe;
use log::{error, info, warn};
use serial_port::{get_serial_port, DataChunk, SerialPort, SerialPortAddress};
use task::{JoinableTaskRef, KillReason};

/// The serial port being used for the default system logger can optionally
/// ignore inputs.
static IGNORED_SERIAL_PORT_INPUT: AtomicU16 = AtomicU16::new(0);

/// Configures the console connection listener to ignore inputs from the given
/// serial port.
///
/// Only one serial port can be ignored, typically the one used for system
/// logging.
pub fn ignore_serial_port_input(serial_port_address: u16) {
    IGNORED_SERIAL_PORT_INPUT.store(serial_port_address, Ordering::Relaxed)
}

/// Starts a new task that detects new console connections
/// by waiting for new data to be received on serial ports.
///
/// Returns the newly-spawned detection task.
pub fn start_connection_detection() -> Result<JoinableTaskRef, &'static str> {
    let (sender, receiver) = async_channel::new_channel(4);
    serial_port::set_connection_listener(sender);

    spawn::new_task_builder(console_connection_detector, receiver)
        .name("console_connection_detector".into())
        .spawn()
}

/// The entry point for the console connection detector task.
fn console_connection_detector(
    connection_listener: Receiver<SerialPortAddress>,
) -> Result<(), &'static str> {
    loop {
        let serial_port_address = connection_listener.receive().map_err(|e| {
            error!("Error receiving console connection request: {:?}", e);
            "error receiving console connection request"
        })?;

        if IGNORED_SERIAL_PORT_INPUT.load(Ordering::Relaxed) == serial_port_address as u16 {
            warn!(
				"Currently ignoring inputs on serial port {:?}. \
				 \n --> Note: QEMU is forwarding control sequences (like Ctrl+C) to Theseus. To exit QEMU, press Ctrl+A then X.",
				serial_port_address,
			);
            continue;
        }

        let serial_port = match get_serial_port(serial_port_address) {
            Some(sp) => sp.clone(),
            _ => {
                error!(
                    "Serial port {:?} was not initialized, skipping console connection request",
                    serial_port_address
                );
                continue;
            }
        };

        let (sender, receiver) = async_channel::new_channel(16);
        if serial_port.lock().set_data_sender(sender).is_err() {
            warn!(
                "Serial port {:?} already had a data sender, skipping console connection request",
                serial_port_address
            );
            continue;
        }

        if spawn::new_task_builder(shell_loop, (serial_port, serial_port_address, receiver))
            .name(format!("{serial_port_address:?}_manager"))
            .spawn()
            .is_err()
        {
            warn!(
                "failed to spawn manager for serial port {:?}",
                serial_port_address
            );
        }
    }
}

fn shell_loop(
    (port, address, receiver): (
        Arc<MutexIrqSafe<SerialPort>>,
        SerialPortAddress,
        Receiver<DataChunk>,
    ),
) -> Result<(), &'static str> {
    info!("creating new tty for serial port {:?}", address);

    let tty = tty::Tty::new();

    let reader_task = spawn::new_task_builder(tty_to_port_loop, (port.clone(), tty.master()))
        .name(format!("tty_to_{address:?}"))
        .spawn()?;
    let writer_task = spawn::new_task_builder(port_to_tty_loop, (receiver, tty.master()))
        .name(format!("{address:?}_to_tty"))
        .spawn()?;

    let new_app_ns = mod_mgmt::create_application_namespace(None)?;

    let (app_file, _ns) =
        mod_mgmt::CrateNamespace::get_crate_object_file_starting_with(&new_app_ns, "hull-")
            .expect("Couldn't find shell in default app namespace");

    let path = path::Path::new(app_file.lock().get_absolute_path());
    let task = spawn::new_application_task_builder(path, Some(new_app_ns))?
        .name(format!("{address:?}_shell"))
        .block()
        .spawn()?;

    let id = task.id;
    let stream = Arc::new(tty.slave());
    app_io::insert_child_streams(
        id,
        app_io::IoStreams {
            discipline: Some(stream.discipline()),
            stdin: stream.clone(),
            stdout: stream.clone(),
            stderr: stream,
        },
    );

    task.unblock().map_err(|_| "couldn't unblock shell task")?;
    task.join()?;

    reader_task.kill(KillReason::Requested).unwrap();
    writer_task.kill(KillReason::Requested).unwrap();

    // Flush the tty in case the reader task didn't run between the last time the
    // shell wrote something to the slave end and us killing the task.
    let mut data = [0; 256];
    if let Ok(len) = tty.master().try_read(&mut data) {
        port.lock()
            .write(&data[..len])
            .map_err(|_| "couldn't write to serial port")?;
    };

    // TODO: Close port?

    Ok(())
}

fn tty_to_port_loop((port, master): (Arc<MutexIrqSafe<SerialPort>>, tty::Master)) {
    let mut data = [0; 256];
    loop {
        let len = match master.read(&mut data) {
            Ok(l) => l,
            Err(e) => {
                error!("couldn't read from master: {e}");
                continue;
            }
        };

        if let Err(e) = port.lock().write(&data[..len]) {
            error!("couldn't write to port: {e}");
        }
    }
}

fn port_to_tty_loop((receiver, master): (Receiver<DataChunk>, tty::Master)) {
    loop {
        let DataChunk { data, len } = match receiver.receive() {
            Ok(d) => d,
            Err(e) => {
                error!("couldn't read from port: {e:?}");
                continue;
            },
        };

        if let Err(e) = master.write(&data[..len as usize]) {
            error!("couldn't write to master: {e}");
        }
    }
}
