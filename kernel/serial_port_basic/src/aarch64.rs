use memory::{MappedPages, PAGE_SIZE, map_frame_range, MMIO_FLAGS};
use super::{TriState, SerialPortInterruptEvent};
use arm_boards::BOARD_CONFIG;
use pl011::PL011;
use core::fmt;

/// The base port I/O addresses for COM serial ports.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum SerialPortAddress {
    /// The base MMIO address for the COM1 serial port.
    COM1 = 0,
    /// The base MMIO address for the COM2 serial port.
    COM2 = 1,
    /// The base MMIO address for the COM3 serial port.
    COM3 = 2,
    /// The base MMIO address for the COM4 serial port.
    COM4 = 3,
}

/// A serial port and its various data and control registers.
pub struct SerialPort {
    port_address: SerialPortAddress,
    inner: Option<PL011>,
    // Owner of the MMIO frames for the PL011 registers
    _mapped_pages: Option<MappedPages>,
}

impl Drop for SerialPort {
    fn drop(&mut self) {
        let sp = self.port_address.to_static_port();
        let mut sp_locked = sp.lock();
        if let TriState::Taken = &*sp_locked {
            let dummy = SerialPort {
                inner: None,
                _mapped_pages: None,
                port_address: self.port_address,
            };
            let dropped = core::mem::replace(self, dummy);
            *sp_locked = TriState::Inited(dropped);
        }
    }
}

impl SerialPort {
    /// Creates and returns a new serial port structure.
    /// 
    /// The configuration parameters of the serial port aren't set by
    /// this function.
    ///
    /// Note: This constructor allocates memory pages and frames; make
    /// sure to initialize the memory subsystem before using it.
    pub fn new(serial_port_address: SerialPortAddress) -> SerialPort {
        let index = serial_port_address as usize;
        let mmio_base = match BOARD_CONFIG.pl011_base_addresses.get(index) {
            Some(addr) => addr,
            None => panic!("Board doesn't have {:?}", serial_port_address),
        };

        let mapped_pages = map_frame_range(*mmio_base, PAGE_SIZE, MMIO_FLAGS)
            .expect("serial_port_basic: couldn't map the UART interface");
        let addr = mapped_pages.start_address().value();
        let mut pl011 = PL011::new(addr as *mut _);

        pl011.enable_rx_interrupt(true);
        pl011.set_fifo_mode(false);
        // pl011.log_status();

        SerialPort {
            port_address: serial_port_address,
            inner: Some(pl011),
            _mapped_pages: Some(mapped_pages),
        }
    }

    /// Enable or disable interrupts on this serial port for various events.
    ///
    /// Note: only [`SerialPortInterruptEvent::DataReceived`] is supported on `aarch64`.
    pub fn enable_interrupt(&mut self, event: SerialPortInterruptEvent, enable: bool) {
        if matches!(event, SerialPortInterruptEvent::DataReceived) {
            self.inner.as_mut().unwrap().enable_rx_interrupt(enable);
        } else {
            unimplemented!()
        }
    }

    /// Write the given string to the serial port, blocking until data can be transmitted.
    ///
    /// # Special characters
    /// Because this function writes strings, it will transmit a carriage return `'\r'`
    /// after transmitting a line feed (new line) `'\n'` to ensure a proper new line.
    pub fn out_str(&mut self, s: &str) {
        self.out_bytes(s.as_bytes())
    }

    /// Write the given byte to the serial port, blocking until data can be transmitted.
    ///
    /// This writes the byte directly with no special cases, e.g., new lines.
    pub fn out_byte(&mut self, byte: u8) {
        while !self.ready_to_transmit() { }
        self.inner.as_mut().unwrap().write_byte(byte);
    }

    /// Write the given bytes to the serial port, blocking until data can be transmitted.
    ///
    /// This writes the bytes directly with no special cases, e.g., new lines.
    pub fn out_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.out_byte(*byte);
        }
    }

    /// Read one byte from the serial port, blocking until data is available.
    pub fn in_byte(&mut self) -> u8 {
        self.inner.as_mut().unwrap().read_byte()
    }

    /// Reads multiple bytes from the serial port into the given `buffer`, non-blocking.
    ///
    /// The buffer will be filled with as many bytes as are available in the serial port.
    /// Once data is no longer available to be read, the read operation will stop.
    ///
    /// If no data is immediately available on the serial port, this will read nothing and return `0`.
    ///
    /// Returns the number of bytes read into the given `buffer`.
    pub fn in_bytes(&mut self, buffer: &mut [u8]) -> usize {
        self.inner.as_mut().unwrap().read_bytes(buffer)
    }

    /// Returns `true` if the serial port is ready to transmit a byte.
    #[inline(always)]
    pub fn ready_to_transmit(&self) -> bool {
        self.inner.as_ref().unwrap().is_writeable()
    }

    /// Returns `true` if the serial port has data available to read.
    #[inline(always)]
    pub fn data_available(&self) -> bool {
        self.inner.as_ref().unwrap().has_incoming_data()
    }

    pub fn base_port_address(&self) -> SerialPortAddress {
        self.port_address
    }

}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.inner.as_mut().unwrap().write_str(s)
    }
}
