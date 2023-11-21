//! Driver for pl011 UARTs

#![no_std]
use core::fmt;
use zerocopy::FromBytes;
use volatile::{Volatile, ReadOnly, WriteOnly};
use memory::{BorrowedMappedPages, Mutable, PhysicalAddress, PAGE_SIZE, map_frame_range, MMIO_FLAGS};

/// Struct representing Pl011 registers. Not intended to be directly used
#[derive(Debug, FromBytes)]
#[repr(C)]
pub struct Pl011_Regs {
    /// Data Register
    pub uartdr: Volatile<u32>,
    /// receive status / error clear
    pub uartrsr: Volatile<u32>,
    reserved0: [u32; 4],
    /// flag register
    pub uartfr: ReadOnly<u32>,
    reserved1: u32,
    /// IrDA Low power counter register
    pub uartilpr: Volatile<u32>,
    /// integer baud rate
    pub uartibrd: Volatile<u32>,
    /// fractional baud rate
    pub uartfbrd: Volatile<u32>,
    /// line control
    pub uartlcr_h: Volatile<u32>,
    /// control
    pub uartcr: Volatile<u32>,
    /// interrupt fifo level select
    pub uartifls: Volatile<u32>,
    /// interrupt mask set/clear
    pub uartimsc: Volatile<u32>,
    /// raw interrupt status
    pub uartris: ReadOnly<u32>,
    /// masked interrupt status
    pub uartmis: ReadOnly<u32>,
    /// interrupt clear
    pub uarticr: WriteOnly<u32>,
    /// dma control
    pub uartdmacr: Volatile<u32>,
    reserved2: [u32; 997],
    /// UART Periph ID0
    pub uartperiphid0: ReadOnly<u32>,
    /// UART Periph ID1
    pub uartperiphid1: ReadOnly<u32>,
    /// UART Periph ID2
    pub uartperiphid2: ReadOnly<u32>,
    /// UART Periph ID3
    pub uartperiphid3: ReadOnly<u32>,
    /// UART PCell ID0
    pub uartpcellid0: ReadOnly<u32>,
    /// UART PCell ID1
    pub uartpcellid1: ReadOnly<u32>,
    /// UART PCell ID2
    pub uartpcellid2: ReadOnly<u32>,
    /// UART PCell ID3
    pub uartpcellid3: ReadOnly<u32>,
}

const UARTIMSC_RXIM: u32 = 1 << 4;
const UARTUCR_RXIC: u32 = 1 << 4;

const UARTLCR_FEN: u32 = 1 << 4;

const UARTCR_RX_ENABLED: u32 = 1 << 9;
const UARTCR_TX_ENABLED: u32 = 1 << 8;
const UARTCR_UART_ENABLED: u32 = 1 << 0;

const UARTFR_RX_BUF_EMPTY: u32 = 1 << 4;
const UARTFR_TX_BUF_FULL: u32 = 1 << 5;

/// A Pl011 Single-Serial-Port Controller.
pub struct Pl011 {
    regs: BorrowedMappedPages<Pl011_Regs, Mutable>
}

impl core::fmt::Debug for Pl011 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.regs.fmt(f)
    }
}

/// Generic methods
impl Pl011 {
    /// Initialize a UART driver.
    pub fn new(base: PhysicalAddress) -> Result<Self, &'static str> {
        let mapped_pages = map_frame_range(base, PAGE_SIZE, MMIO_FLAGS)?;

        let mut this = Self {
            regs: mapped_pages.into_borrowed_mut(0).map_err(|(_, e)| e)?,
        };

        this.enable_rx_interrupt(true);
        this.set_fifo_mode(false);

        Ok(this)
    }

    /// Enable on-receive interrupt
    pub fn enable_rx_interrupt(&mut self, enable: bool) {
        let mut reg = self.regs.uartimsc.read();

        match enable {
            true  => reg |=  UARTIMSC_RXIM,
            false => reg &= !UARTIMSC_RXIM,
        };

        self.regs.uartimsc.write(reg);
    }

    pub fn acknowledge_rx_interrupt(&mut self) {
        self.regs.uarticr.write(UARTUCR_RXIC);
    }

    /// Set FIFO mode
    pub fn set_fifo_mode(&mut self, enable: bool) {
        let mut reg = self.regs.uartlcr_h.read();

        match enable {
            true  => reg |=  UARTLCR_FEN,
            false => reg &= !UARTLCR_FEN,
        };

        self.regs.uartlcr_h.write(reg);
    }

    /// Outputs a summary of the state of the controller using `log::info!()`
    pub fn log_status(&self) {
        let reg = self.regs.uartcr.read();
        log::info!("RX enabled: {}", (reg & UARTCR_RX_ENABLED) > 0);
        log::info!("TX enabled: {}", (reg & UARTCR_TX_ENABLED) > 0);
        log::info!("UART enabled: {}", (reg & UARTCR_UART_ENABLED) > 0);
    }

    /// Returns true if the receive-buffer-empty flag is clear.
    pub fn has_incoming_data(&self) -> bool {
        let uartfr = self.regs.uartfr.read();
        uartfr & UARTFR_RX_BUF_EMPTY == 0
    }

    /// Reads a single byte out the uart
    ///
    /// Spins until a byte is available in the fifo.
    pub fn read_byte(&self) -> u8 {
        while !self.has_incoming_data() {}
        self.regs.uartdr.read() as u8
    }

    /// Reads bytes into a slice until there is none available.
    pub fn read_bytes(&self, bytes: &mut [u8]) -> usize {
        let mut read = 0;

        while read < bytes.len() && self.has_incoming_data() {
            bytes[read] = self.read_byte();
            read += 1;
        }

        read
    }

    /// Returns true if the transmit-buffer-full flag is clear.
    pub fn is_writeable(&self) -> bool {
        let uartfr = self.regs.uartfr.read();
        uartfr & UARTFR_TX_BUF_FULL == 0
    }

    /// Writes a single byte out the uart.
    ///
    /// Spins until space is available in the fifo.
    pub fn write_byte(&mut self, data: u8) {
        while !self.is_writeable() {}
        self.regs.uartdr.write(data as u32);
    }

    /// Writes a byte slice out the uart.
    ///
    /// Spins until space is available in the fifo.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_byte(*b);
        }
    }
}

impl fmt::Write for Pl011 {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_bytes(s.as_bytes());
        Ok(())
    }
}
