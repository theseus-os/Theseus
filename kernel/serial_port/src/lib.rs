//! A full serial driver with more advanced I/O support, e.g., interrupt-based data receival.
//!
//! This crate builds on  [`serial_port_basic`], which provides the lower-level types
//! and functions that enable simple interactions with serial ports. 
//! This crate extends that functionality to provide interrupt handlers for receiving data
//! and handling data access in a deferred, asynchronous manner.
//! It also implements additional higher-level I/O traits for serial ports,
//! namely [`core2::io::Read`] and [`core2::io::Write`].
//!
//! # Notes
//! Typically, drivers do not need to be designed in this split manner. 
//! However, the serial port is the very earliest device to be initialized and used
//! in Theseus, as it acts as the backend output stream for Theseus's logger.

#![no_std]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate spin;
extern crate irq_safety;
extern crate interrupts;
extern crate deferred_interrupt_tasks;
extern crate core2;
extern crate x86_64;
extern crate serial_port_basic;

use deferred_interrupt_tasks::InterruptRegistrationError;
pub use serial_port_basic::{
    SerialPortAddress,
    SerialPortInterruptEvent,
    SerialPort as SerialPortBasic,
    take_serial_port as take_serial_port_basic,
};

use alloc::{boxed::Box, sync::Arc};
use core::{convert::TryFrom, fmt, ops::{Deref, DerefMut}};
use irq_safety::MutexIrqSafe;
use spin::Once;
use interrupts::IRQ_BASE_OFFSET;
use x86_64::structures::idt::{HandlerFunc, InterruptStackFrame};

// Dependencies below here are temporary and will be removed
// after we have support for separate interrupt handling tasks.
extern crate async_channel;
use async_channel::Sender;

/// A temporary hack to allow the serial port interrupt handler
/// to inform a listener on the other end of this channel
/// that a new connection has been detected on one of the serial ports,
/// i.e., that it received some data on a serial port that 
/// didn't expect it or wasn't yet set up to handle incoming data.
pub fn set_connection_listener(
    sender: Sender<SerialPortAddress>
) -> &'static Sender<SerialPortAddress> {
    NEW_CONNECTION_NOTIFIER.call_once(|| sender)
}
static NEW_CONNECTION_NOTIFIER: Once<Sender<SerialPortAddress>> = Once::new();


// Serial ports cannot be reliably probed (discovered dynamically), thus,
// we ensure they are exposed safely as singletons through the below static instances.
static COM1_SERIAL_PORT: Once<Arc<MutexIrqSafe<SerialPort>>> = Once::new();
static COM2_SERIAL_PORT: Once<Arc<MutexIrqSafe<SerialPort>>> = Once::new();
static COM3_SERIAL_PORT: Once<Arc<MutexIrqSafe<SerialPort>>> = Once::new();
static COM4_SERIAL_PORT: Once<Arc<MutexIrqSafe<SerialPort>>> = Once::new();


/// Obtains a reference to the [`SerialPort`] specified by the given [`SerialPortAddress`],
/// if it has been initialized (see [`init_serial_port()`]).
pub fn get_serial_port(
    serial_port_address: SerialPortAddress
) -> Option<&'static Arc<MutexIrqSafe<SerialPort>>> {
    static_port_of(&serial_port_address).get()
}

/// Initializes the [`SerialPort`] specified by the given [`SerialPortAddress`].
///
/// This function also registers the interrupt handler for this serial port
/// such that it can receive data using interrupts instead of busy-waiting or polling.
///
/// If the given serial port has already been initialized, this does nothing
/// and simply returns a reference to the already-initialized serial port.
pub fn init_serial_port(
    serial_port_address: SerialPortAddress,
    serial_port: SerialPortBasic,
) -> &'static Arc<MutexIrqSafe<SerialPort>> {
    static_port_of(&serial_port_address).call_once(|| {
        let sp = Arc::new(MutexIrqSafe::new(SerialPort::new(serial_port)));
        let (int_num, int_handler) = interrupt_number_handler(&serial_port_address);
        SerialPort::register_interrupt_handler(sp.clone(), int_num, int_handler).unwrap();
        sp
    })
}

/// Returns a reference to the static instance of this serial port.
fn static_port_of(
    serial_port_address: &SerialPortAddress
) -> &'static Once<Arc<MutexIrqSafe<SerialPort>>> {
    match serial_port_address {
        SerialPortAddress::COM1 => &COM1_SERIAL_PORT,
        SerialPortAddress::COM2 => &COM2_SERIAL_PORT,
        SerialPortAddress::COM3 => &COM3_SERIAL_PORT,
        SerialPortAddress::COM4 => &COM4_SERIAL_PORT,
    }
}

/// Returns the interrupt number (IRQ vector)
/// and the interrupt handler function for this serial port.
fn interrupt_number_handler(
    serial_port_address: &SerialPortAddress
) -> (u8, HandlerFunc) {
    match serial_port_address {
        SerialPortAddress::COM1 | SerialPortAddress::COM3 => (IRQ_BASE_OFFSET + 0x04, com1_com3_interrupt_handler),
        SerialPortAddress::COM2 | SerialPortAddress::COM4 => (IRQ_BASE_OFFSET + 0x03, com2_com4_interrupt_handler),
    }
}


/// A serial port abstraction with support for interrupt-based data receival.
pub struct SerialPort {
    /// The basic interface used to access this serial port.
    inner: SerialPortBasic,
    /// The channel endpoint to which data received on this serial port will be pushed.
    /// If `None`, received data will be ignored and a warning printed.
    /// 
    /// The format of data sent via this channel is effectively a slice of bytes,
    /// but is represented without using references as a tuple:
    ///  * the number of bytes actually being transmitted, to be used as an index into the array,
    ///  * an array of bytes holding the actual data, up to 
    data_sender: Option<Sender<DataChunk>>,
}
impl Deref for SerialPort {
    type Target = SerialPortBasic;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl DerefMut for SerialPort {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl SerialPort {
    /// Initialize this serial port by giving it ownership and control of
    /// the given basic `serial_port`.
    pub fn new(serial_port: SerialPortBasic) -> SerialPort {
        SerialPort {
            inner: serial_port,
            data_sender: None,
        }
    }

    /// Register the interrupt handler for this serial port
    /// and spawn a deferrent interrupt task to handle its data receival. 
    pub fn register_interrupt_handler(
        serial_port: Arc<MutexIrqSafe<SerialPort>>,
        interrupt_number: u8,
        interrupt_handler: HandlerFunc,
    ) -> Result<(), &'static str> {
        let base_port = { 
            let sp = serial_port.lock();
            sp.base_port_address()
        };

        // Register the interrupt handler for this serial port. 
        let registration_result = deferred_interrupt_tasks::register_interrupt_handler(
            interrupt_number,
            interrupt_handler,
            serial_port_receive_deferred,
            serial_port,
            Some(format!("serial_port_deferred_task_irq_{interrupt_number:#X}")),
        );

        match registration_result {
            Ok(deferred_task) => {
                // Now that we successfully registered the interrupt and spawned 
                // a deferred interrupt task, save some information for the 
                // immediate interrupt handler to use when it fires
                // such that it triggers the deferred task to act. 
                info!("Registered interrupt handler at IRQ {:#X} for serial port {:#X}.", 
                    interrupt_number, base_port,
                );
                match SerialPortAddress::try_from(base_port) {
                    Ok(SerialPortAddress::COM1 | SerialPortAddress::COM3) => {
                        INTERRUPT_ACTION_COM1_COM3.call_once(|| 
                            Box::new(move || {
                                deferred_task.unblock()
                                    .expect("BUG: com_1_com3_interrupt_handler: couldn't unblock deferred task");
                            })
                        );
                    }
                    Ok(SerialPortAddress::COM2 | SerialPortAddress::COM4) => {
                        INTERRUPT_ACTION_COM2_COM4.call_once(|| 
                            Box::new(move || {
                                deferred_task.unblock()
                                    .expect("BUG: com_2_com4_interrupt_handler: couldn't unblock deferred task");
                            })
                        );
                    }
                    Err(_) => warn!("Registering interrupt handler for unknown serial port at {:#X}", base_port),
                };                
            }
            Err(InterruptRegistrationError::IrqInUse { irq, existing_handler_address }) => {
                if existing_handler_address != interrupt_handler as usize {
                    error!("Failed to register interrupt handler at IRQ {:#X} for serial port {:#X}. \
                        Existing interrupt handler was a different handler, at address {:#X}.",
                        irq, base_port, existing_handler_address,
                    );
                }
            }
            Err(InterruptRegistrationError::SpawnError(e)) => return Err(e),
        }

        Ok(())
    }


    /// Tells this `SerialPort` to push received data bytes
    /// onto the given `sender` channel.
    ///
    /// If a sender already exists for this serial port,
    /// the existing sender is *not* replaced and an error is returned.
    pub fn set_data_sender(
        &mut self,
        sender: Sender<DataChunk>
    ) -> Result<(), DataSenderAlreadyExists> {
        if self.data_sender.is_some() { 
            Err(DataSenderAlreadyExists)
        } else {
            self.data_sender = Some(sender);
            Ok(())
        }
    }

}

/// An empty error type indicating that a data sender could not be set
/// for a serial port because a sender had already been set for it.
#[derive(Debug)]
pub struct DataSenderAlreadyExists;


/// A non-blocking implementation of [`core2::io::Read`] that will read bytes into the given `buf`
/// so long as more bytes are available.
/// The read operation will be completed when there are no more bytes to be read,
/// or when the `buf` is filled, whichever comes first.
///
/// Because it's non-blocking, a [`core2::io::ErrorKind::WouldBlock`] error is returned
/// if there are no bytes available to be read, indicating that the read would block.
impl core2::io::Read for SerialPort {
    fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize> {
        if !self.data_available() {
            return Err(core2::io::ErrorKind::WouldBlock.into());
        }
        Ok(self.in_bytes(buf))
    }
}

/// A blocking implementation of [`core2::io::Write`] that will write bytes from the given `buf`
/// to the `SerialPort`, waiting until it is ready to transfer all bytes. 
///
/// The `flush()` function is a no-op, since the `SerialPort` does not have buffering. 
impl core2::io::Write for SerialPort {
    fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize> {
        self.out_bytes(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> core2::io::Result<()> {
        Ok(())
    }    
}

/// Forward the implementation of [`core::fmt::Write`] to the inner [`SerialPortBasic`].
impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.inner.write_str(s) 
    }
}


/// This function is invoked from the serial port's deferred interrupt task,
/// and runs asynchronously after a serial port interrupt has occurred. 
///
/// Currently, we only use interrupts for receiving data on a serial port.
///
/// This is responsible for actually reading the received data from the serial port
/// and doing something with that data.
/// On the other hand, the interrupt handler itself merely notifies the system 
/// that it's time to invoke this function soon.
fn serial_port_receive_deferred(
    serial_port: &Arc<MutexIrqSafe<SerialPort>>
) -> Result<(), ()> {
    let mut buf = DataChunk::empty();
    let bytes_read;
    let base_port;
    
    let mut input_was_ignored = false;
    let mut send_result = Ok(());

    // We shouldn't hold the serial port lock for long periods of time,
    // and we cannot hold it at all while issuing a log statement.
    { 
        let mut sp = serial_port.lock();
        base_port = sp.base_port_address();
        bytes_read = sp.in_bytes(&mut buf.data);
        if bytes_read > 0 {
            if let Some(ref sender) = sp.data_sender {
                buf.len = bytes_read as u8;
                send_result = sender.try_send(buf);
            } else {
                input_was_ignored = true;
            }
        } else {
            // Ignore this interrupt, as it was caused by a `SerialPortInterruptEvent` 
            // other than data being received, which is the only one we currently care about.
            return Ok(());
        }
    }

    if let Err(e) = send_result {
        error!("Failed to send data received for serial port at {:#X}: {:?}.", base_port, e.1);
    }

    if input_was_ignored {
        if let Some(sender) = NEW_CONNECTION_NOTIFIER.get() {
            // info!("Requesting new console to be spawned for this serial port ({:#X})", base_port);
            if let Ok(serial_port_address) = SerialPortAddress::try_from(base_port) {
                if let Err(err) = sender.try_send(serial_port_address) {
                    error!("Error sending request for new console to be spawned for this serial port ({:#X}): {:?}",
                        base_port, err
                    );
                }
            } else {
                error!("Error: base port {:#X} was not a known serial port address.", base_port);
            }
        } else {
            warn!("Warning: no connection detector; ignoring {}-byte input read from serial port {:#X}.",
                bytes_read, base_port
            );
        }
    }

    Ok(())
}

/// A chunk of data read from a serial port that will be transmitted to a receiver.
///
/// For performance, this type is sized to and aligned to 64-byte boundaries 
/// such that it fits in a cache line. 
#[repr(align(64))]
pub struct DataChunk {
    pub len: u8,
    pub data: [u8; (64 - 1)],
}
const _: () = assert!(core::mem::size_of::<DataChunk>() == 64);
const _: () = assert!(core::mem::align_of::<DataChunk>() == 64);

impl DataChunk {
    /// Returns a new `DataChunk` filled with zeroes that can be written into.
    pub const fn empty() -> Self {
        DataChunk { len: 0, data: [0; (64 - 1)] }
    }
}

/// A closure specifying the action that will be taken when a serial port interrupt occurs
/// (for COM1 or COM3 interrupts, since they share an IRQ line).
static INTERRUPT_ACTION_COM1_COM3: Once<Box<dyn Fn() + Send + Sync>> = Once::new();
/// A closure specifying the action that will be taken when a serial port interrupt occurs
/// (for COM2 or COM4 interrupts, since they share an IRQ line).
static INTERRUPT_ACTION_COM2_COM4: Once<Box<dyn Fn() + Send + Sync>> = Once::new();


/// IRQ 0x24: COM1 and COM3 serial port interrupt handler.
extern "x86-interrupt" fn com1_com3_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // trace!("COM1/COM3 serial handler");
    if let Some(func) = INTERRUPT_ACTION_COM1_COM3.get() {
        func();
    }
    interrupts::eoi(Some(IRQ_BASE_OFFSET + 0x4));
}

/// IRQ 0x23: COM2 and COM4 serial port interrupt handler.
extern "x86-interrupt" fn com2_com4_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // trace!("COM2/COM4 serial handler");
    if let Some(func) = INTERRUPT_ACTION_COM2_COM4.get() {
        func();
    }
    interrupts::eoi(Some(IRQ_BASE_OFFSET + 0x3));
}
