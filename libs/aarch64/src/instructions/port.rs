//! I/O port functionality.

/// Write 8 bits to I/O port.
pub unsafe fn outb(_port: u16, _val: u8) {
    //TODO
}

/// Read 8 bits from I/O port.
pub unsafe fn inb(_port: u16) -> u8 {
    //TODO
        0
}

/// Write 16 bits to I/O port.
pub unsafe fn outw(_port: u16, _val: u16) {
    //TODO
}

/// Read 16 bits from I/O port.
pub unsafe fn inw(_port: u16) -> u16 {
    //TODO
        0
}

/// Write 32 bits to I/O port.
pub unsafe fn outl(_port: u16, _val: u32) {
    //TODO
}

/// Read 32 bits from I/O port.
pub unsafe fn inl(_port: u16) -> u32 {
    //TODO
        0
}

/// Write 8-bit array to I/O port.
pub unsafe fn outsb(_port: u16, _buf: &[u8]) {
    //TODO
}

/// Read 8-bit array from I/O port.
pub unsafe fn insb(_port: u16, _buf: &mut [u8]) {
    //TODO
}

/// Write 16-bit array to I/O port.
pub unsafe fn outsw(_port: u16, _buf: &[u16]) {
    //TODO
}

/// Read 16-bit array from I/O port.
pub unsafe fn insw(_port: u16, _buf: &mut [u16]) {
    //TODO
}

/// Write 32-bit array to I/O port.
pub unsafe fn outsl(_port: u16, _buf: &[u32]) {
    //TODO
}

/// Read 32-bit array from I/O port.
pub unsafe fn insl(_port: u16, _buf: &mut [u32]) {
    //TODO
}
