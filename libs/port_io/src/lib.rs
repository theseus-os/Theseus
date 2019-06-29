//! CPU-level input/output instructions, including `inb`, `outb`, etc., and
//! a high level Rust wrapper.

#![feature(asm, const_fn)]
#![no_std]

use core::marker::PhantomData;

// These cfg statements should cause compiler errors on non-x86 platforms.
#[cfg(any(target_arch="x86", target_arch="x86_64"))]
mod x86;

#[cfg(any(target_arch="x86", target_arch="x86_64"))]
pub use x86::{inb, outb, inw, outw, inl, outl};


/// This trait is defined for any type which can be read or written over a port.
/// x86 processors support Port IO for `u8`, `u16` and `u32`.
pub trait InOut {
    /// Read a value from the specified port.
    unsafe fn port_in(port: u16) -> Self;

    /// Write a value to the specified port.
    unsafe fn port_out(port: u16, value: Self);
}

impl InOut for u8 {
    unsafe fn port_in(port: u16) -> u8 { inb(port) }
    unsafe fn port_out(port: u16, value: u8) { outb(value, port); }
}

impl InOut for u16 {
    unsafe fn port_in(port: u16) -> u16 { inw(port) }
    unsafe fn port_out(port: u16, value: u16) { outw(value, port); }
}

impl InOut for u32 {
    unsafe fn port_in(port: u16) -> u32 { inl(port) }
    unsafe fn port_out(port: u16, value: u32) { outl(value, port); }
}


/// A readable and writable I/O port over an arbitrary type supporting the `InOut` interface,
/// which is only `u8`, `u16`, and `u32`.
#[derive(Debug)]
pub struct Port<T: InOut> {
    /// Port address (in the I/O space), which is always 16 bits.
    port: u16,
    _phantom: PhantomData<T>,
}
impl<T: InOut> Port<T> {
    /// Create a new I/O port.
    pub const fn new(port: u16) -> Port<T> {
        Port { port: port, _phantom: PhantomData }
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
pub struct PortReadOnly<T: InOut> {
    /// Port address (in the I/O space), which is always 16 bits.
    port: u16,
    _phantom: PhantomData<T>,
}
impl<T: InOut> PortReadOnly<T> {
    /// Create a new read-only I/O port.
    pub const fn new(port: u16) -> PortReadOnly<T> {
        PortReadOnly { port: port, _phantom: PhantomData }
    }

    /// Read data of size `T` from the port.
    pub fn read(&self) -> T {
        unsafe { T::port_in(self.port) }
    }
}


/// A write-only I/O port over an arbitrary type supporting the `InOut` interface,
/// which is only `u8`, `u16`, and `u32`.
#[derive(Debug)]
pub struct PortWriteOnly<T: InOut> {
    /// Port address (in the I/O space), which is always 16 bits.
    port: u16,
    _phantom: PhantomData<T>,
}
impl<T: InOut> PortWriteOnly<T> {
    /// Create a new read-only I/O port.
    pub const fn new(port: u16) -> PortWriteOnly<T> {
        PortWriteOnly { port: port, _phantom: PhantomData }
    }

    /// Write data of size `T` to the port.
    /// This is unsafe because writing to an arbitrary port may cause problems.
    pub unsafe fn write(&self, value: T) {
        T::port_out(self.port, value);
    }
}
