//! A full serial driver with more advanced I/O support, e.g., interrupt-based data receival.
//!
//! This crate is a wrapper around [`serial_port_basic`], which provides the lower-level types
//! and functions that enable simple interactions with serial ports. 
//! This crate extends that functionality to provide interrupt handlers for receiving data
//! and handling data access in a deferred, asynchronous manner.
//! It also implements higher-level I/O traits for serial ports,
//! namely [`bare_io::Read`] and [`bare_io::Write`].
//!
//! # Notes
//! Typically, drivers do not need to be designed in this split manner. 
//! However, the serial port is the very earliest device to be initialized and used
//! in Theseus, as it acts as the backend output stream for Theseus's logger.

#![no_std]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate irq_safety;
extern crate interrupts;
extern crate bare_io;
extern crate x86_64;
extern crate serial_port_basic;

pub use serial_port_basic::{
    SerialPortAddress,
    SerialPort as SerialPortBasic,
    take_serial_port as take_serial_port_basic,
};

use alloc::sync::Arc;
use core::{fmt, ops::{Deref, DerefMut}};
use irq_safety::MutexIrqSafe;
use spin::Once;
use interrupts::{IRQ_BASE_OFFSET, register_interrupt};
use x86_64::structures::idt::{HandlerFunc, ExceptionStackFrame};

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
/// If the given serial port has already been initialized, this does nothing.
pub fn init_serial_port(
    serial_port_address: SerialPortAddress,
    serial_port: SerialPortBasic,
) -> &'static Arc<MutexIrqSafe<SerialPort>> {
    static_port_of(&serial_port_address).call_once(|| {
        let mut sp = SerialPort::new(serial_port);
        let (int_num, int_handler) = interrupt_number_handler(&serial_port_address);
        sp.register_interrupt_handler(int_num, int_handler).unwrap();
        Arc::new(MutexIrqSafe::new(sp))
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
        SerialPortAddress::COM1 | SerialPortAddress::COM3 => (IRQ_BASE_OFFSET + 0x04, com1_serial_handler),
        SerialPortAddress::COM2 | SerialPortAddress::COM4 => (IRQ_BASE_OFFSET + 0x03, com2_serial_handler),
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
        &mut self,
        interrupt_number: u8,
        interrupt_handler: HandlerFunc,
    ) -> Result<(), &'static str> {
        let base_port = self.inner.base_port_address();

        // Register the interrupt handler for this serial port. 
        let res = register_interrupt(interrupt_number, interrupt_handler);
        if let Err(registered_handler_addr) = res {
            if registered_handler_addr != interrupt_handler as u64 {
                error!("Failed to register interrupt handler at IRQ {:#X} for serial port {:#X}. \
                    Existing interrupt handler was at address {:#X}.",
                    interrupt_number, base_port, registered_handler_addr,
                );
            }
        } else {
            info!("Registered interrupt handler at IRQ {:#X} for serial port {:#X}.", 
                interrupt_number, base_port,
            );
        }

        Ok(())
    }


    /// Tells this `SerialPort` to push received data bytes
    /// onto the given `sender` channel.
    ///
    /// If a sender already existed, it is replaced
    /// by the given `sender` and returned.
    pub fn set_data_sender(
        &mut self,
        sender: Sender<DataChunk>
    ) -> Option<Sender<DataChunk>> {
        self.data_sender.replace(sender)
    }

}



/// A non-blocking implementation of [`bare_io::Read`] that will read bytes into the given `buf`
/// so long as more bytes are available.
/// The read operation will be completed when there are no more bytes to be read,
/// or when the `buf` is filled, whichever comes first.
///
/// Because it's non-blocking, a [`bare_io::ErrorKind::WouldBlock`] error is returned
/// if there are no bytes available to be read, indicating that the read would block.
impl bare_io::Read for SerialPort {
    fn read(&mut self, buf: &mut [u8]) -> bare_io::Result<usize> {
        if !self.data_available() {
            return Err(bare_io::ErrorKind::WouldBlock.into());
        }
        Ok(self.in_bytes(buf))
    }
}

/// A blocking implementation of [`bare_io::Write`] that will write bytes from the given `buf`
/// to the `SerialPort`, waiting until it is ready to transfer all bytes. 
///
/// The `flush()` function is a no-op, since the `SerialPort` does not have buffering. 
impl bare_io::Write for SerialPort {
    fn write(&mut self, buf: &[u8]) -> bare_io::Result<usize> {
        self.out_bytes(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> bare_io::Result<()> {
        Ok(())
    }    
}

/// Forward the implementation of [`core::fmt::Write`] to the inner [`SerialPortBasic`].
impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.inner.write_str(s) 
    }
}



/// This is called from the serial port interrupt handlers 
/// when data has been received and is ready to read.
fn handle_receive_interrupt(serial_port_address: SerialPortAddress) {
    // Important notes:
    //  * We read a chunk of multiple bytes at once.
    //  * We MUST NOT use a blocking read operation in an interrupt handler. 
    //  * We cannot hold the serial port lock while issuing a log statement.
    let mut buf = [0; u8::MAX as usize];
    let bytes_read;
    
    let mut input_was_ignored = false;
    let mut send_result = Ok(());
    let serial_port = match get_serial_port(serial_port_address) {
        Some(sp) => sp,
        _ => return,
    };

    { 
        let mut sp = serial_port.lock();
        bytes_read = sp.in_bytes(&mut buf);
        if bytes_read > 0 {
            if let Some(ref sender) = sp.data_sender {
                send_result = sender.try_send((bytes_read as u8, buf));
            } else {
                input_was_ignored = true;
            }
        } else {
            // This was a "false" interrupt, no data was actually received.
            return;
        }
    }

    use fmt::Write;
    if let Err(e) = send_result {
        let _result = write!(
            &mut serial_port.lock(),
            "\x1b[31mError: failed to send data received for serial port {:?}: {:?}.\x1b[0m\n",
            serial_port_address, e.1
        );
    }

    if input_was_ignored {
        if let Some(sender) = NEW_CONNECTION_NOTIFIER.get() {
            let _result = write!(
                &mut serial_port.lock(),
                "\x1b[36mRequesting new console to be spawned for this serial port ({:?})\x1b[0m\n",
                serial_port_address
            );
            if let Err(err) = sender.try_send(serial_port_address) {
                let _result = write!(
                    &mut serial_port.lock(),
                    "\x1b[31mError sending request for new console to be spawned for this serial port ({:?}): {:?}\x1b[0m\n",
                    serial_port_address, err
                );
            }
        } else {
            let _result = write!(
                &mut serial_port.lock(),
                "\x1b[33mWarning: no connection detector; ignoring {}-byte input read from serial port {:?}: {:X?}\x1b[0m\n",
                bytes_read, serial_port_address, &buf[..bytes_read]
            );
        }
    }
}

/// A chunk of data read from a serial port
/// that will be transmitted to a receiver.
pub type DataChunk = (u8, [u8; u8::MAX as usize]);



/// IRQ 0x23: COM2 serial port interrupt handler.
///
/// Note: this IRQ may also be used for COM4, but I haven't seen a machine with a COM4 port yet.
extern "x86-interrupt" fn com2_serial_handler(_stack_frame: &mut ExceptionStackFrame) {
    // trace!("COM2 serial handler");
    handle_receive_interrupt(SerialPortAddress::COM2);
    interrupts::eoi(Some(IRQ_BASE_OFFSET + 0x3));
}


/// IRQ 0x24: COM1 serial port interrupt handler.
///
/// Note: this IRQ may also be used for COM3, but I haven't seen a machine with a COM3 port yet.
extern "x86-interrupt" fn com1_serial_handler(_stack_frame: &mut ExceptionStackFrame) {
    // trace!("COM1 serial handler");
    handle_receive_interrupt(SerialPortAddress::COM1);
    interrupts::eoi(Some(IRQ_BASE_OFFSET + 0x4));
}
