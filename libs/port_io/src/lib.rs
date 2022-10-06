//! CPU-level input/output instructions, including `inb`, `outb`, etc., and
//! a high level Rust wrapper.

#![no_std]

use core::marker::PhantomData;

// These cfg statements should cause compiler errors on non-x86 platforms.
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
mod x86;

#[cfg(any(target_arch="x86", target_arch="x86_64"))]
pub use x86::{inb, outb, inw, outw, inl, outl};


/// This trait is defined for any type which can be read from a port.
/// x86 processors support Port IO for `u8`, `u16` and `u32`.
pub trait PortIn {
    /// Read a value from the specified port.
    unsafe fn port_in(port: u16) -> Self;
}

/// This trait is defined for any type which can be read from a port.
/// x86 processors support Port IO for `u8`, `u16` and `u32`.
pub trait PortOut {
    /// Write a value to the specified port.
    unsafe fn port_out(port: u16, value: Self);
}

impl PortOut for u8 {
    unsafe fn port_out(port: u16, value: Self) {
        outb(value, port);
    }
}
impl PortOut for u16 {
    unsafe fn port_out(port: u16, value: Self) {
        outw(value, port);
    }
}
impl PortOut for u32 {
    unsafe fn port_out(port: u16, value: Self) {
        outl(value, port);
    }
}

impl PortIn for u8 {
    unsafe fn port_in(port: u16) -> Self {
        inb(port)
    }
}
impl PortIn for u16 {
    unsafe fn port_in(port: u16) -> Self {
        inw(port)
    }
}
impl PortIn for u32 {
    unsafe fn port_in(port: u16) -> Self {
        inl(port)
    }
}



/// A readable and writable I/O port over an arbitrary type supporting the `InOut` interface,
/// which is only `u8`, `u16`, or `u32`.
// /// which only includes types that losslessly convert into a `u8`, `u16`, and `u32`.
#[derive(Debug)]
pub struct Port<T: PortIn + PortOut> {
    /// Port address (in the I/O space), which is always 16 bits.
    port: u16,
    _phantom: PhantomData<T>,
}
impl<T: PortIn + PortOut> Port<T> {
    /// Create a new I/O port.
    pub const fn new(port: u16) -> Port<T> {
        Port { port: port, _phantom: PhantomData }
    }

    /// Returns the address of this port.
    pub const fn port_address(&self) -> u16 { 
        self.port
    }

    /// Read data of size `T` from the port.
    pub fn read(&self) -> T {
        unsafe { T::port_in(self.port) }
    }

    /// Write data of size `T` to the port.
    /// This is unsafe because writing to an arbitrary port may cause problems.
    pub unsafe fn write(&self, value: T) {
        T::port_out(self.port, value);
    }
}


/// A read-only I/O port over an arbitrary type supporting the `InOut` interface,
/// which is only `u8`, `u16`, and `u32`.
#[derive(Debug)]
pub struct PortReadOnly<T: PortIn> {
    /// Port address (in the I/O space), which is always 16 bits.
    port: u16,
    _phantom: PhantomData<T>,
}
impl<T: PortIn> PortReadOnly<T> {
    /// Create a new read-only I/O port.
    pub const fn new(port: u16) -> PortReadOnly<T> {
        PortReadOnly { port: port, _phantom: PhantomData }
    }

    /// Returns the address of this port.
    pub const fn port_address(&self) -> u16 { 
        self.port
    }

    /// Read data of size `T` from the port.
    pub fn read(&self) -> T {
        unsafe { T::port_in(self.port) }
    }
}


/// A write-only I/O port over an arbitrary type supporting the `InOut` interface,
/// which is only `u8`, `u16`, and `u32`.
#[derive(Debug)]
pub struct PortWriteOnly<T: PortOut> {
    /// Port address (in the I/O space), which is always 16 bits.
    port: u16,
    _phantom: PhantomData<T>,
}
impl<T: PortOut> PortWriteOnly<T> {
    /// Create a new read-only I/O port.
    pub const fn new(port: u16) -> PortWriteOnly<T> {
        PortWriteOnly { port: port, _phantom: PhantomData }
    }

    /// Returns the address of this port.
    pub const fn port_address(&self) -> u16 { 
        self.port
    }

    /// Write data of size `T` to the port.
    /// This is unsafe because writing to an arbitrary port may cause problems.
    pub unsafe fn write(&self, value: T) {
        T::port_out(self.port, value);
    }
}
