//! Allows stdio to be used with `noline`.

use alloc::sync::Arc;
use app_io::{ImmutableRead, ImmutableWrite};
use core2::io;
use embedded_hal::serial;

pub(crate) struct Wrapper {
    pub(crate) stdin: Arc<dyn ImmutableRead>,
    pub(crate) stdout: Arc<dyn ImmutableWrite>,
}

impl serial::Read<u8> for Wrapper {
    type Error = io::Error;

    fn read(&mut self) -> nb::Result<u8, Self::Error> {
        let mut buf = [0; 1];
        match self.stdin.read(&mut buf)? {
            0 => Err(nb::Error::Other(io::Error::new(
                io::ErrorKind::Other,
                "read zero",
            ))),
            _ => Ok(buf[0]),
        }
    }
}

impl serial::Write<u8> for Wrapper {
    type Error = io::Error;

    fn write(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
        match self.stdout.write(&[byte])? {
            0 => Err(nb::Error::Other(io::ErrorKind::WriteZero.into())),
            _ => Ok(()),
        }
    }

    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        // TODO: Forward to slave?
        Ok(())
    }
}
