//! I/O port functionality.

/// Write 8 bits to I/O port.
pub unsafe fn outb(port: u16, val: u8) {
    //TODO
}

/// Read 8 bits from I/O port.
pub unsafe fn inb(port: u16) -> u8 {
    //TODO
        0
}

/// Write 16 bits to I/O port.
pub unsafe fn outw(port: u16, val: u16) {
    //TODO
}

/// Read 16 bits from I/O port.
pub unsafe fn inw(port: u16) -> u16 {
    //TODO
        0
}

/// Write 32 bits to I/O port.
pub unsafe fn outl(port: u16, val: u32) {
    //TODO
}

/// Read 32 bits from I/O port.
pub unsafe fn inl(port: u16) -> u32 {
    //TODO
        0
}

/// Write 8-bit array to I/O port.
pub unsafe fn outsb(port: u16, buf: &[u8]) {
    //TODO
}

/// Read 8-bit array from I/O port.
pub unsafe fn insb(port: u16, buf: &mut [u8]) {
    //TODO
}

/// Write 16-bit array to I/O port.
pub unsafe fn outsw(port: u16, buf: &[u16]) {
    //TODO
}

/// Read 16-bit array from I/O port.
pub unsafe fn insw(port: u16, buf: &mut [u16]) {
    //TODO
}

/// Write 32-bit array to I/O port.
pub unsafe fn outsl(port: u16, buf: &[u32]) {
    //TODO
}

/// Read 32-bit array from I/O port.
pub unsafe fn insl(port: u16, buf: &mut [u32]) {
    //TODO
}
