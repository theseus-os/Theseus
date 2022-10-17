use alloc::sync::Arc;
use app_io::{ImmutableRead, ImmutableWrite};
use embedded_hal::serial;

pub(crate) struct Wrapper {
    pub(crate) stdin: Arc<dyn ImmutableRead>,
    pub(crate) stdout: Arc<dyn ImmutableWrite>,
}

impl serial::Read<u8> for Wrapper {
    type Error = core2::io::Error;

    fn read(&mut self) -> nb::Result<u8, Self::Error> {
        let mut buf = [0; 1];
        assert_eq!(self.stdin.read(&mut buf)?, 1);
        Ok(buf[0])
    }
}

impl serial::Write<u8> for Wrapper {
    type Error = core2::io::Error;

    fn write(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
        assert_eq!(self.stdout.write(&[byte])?, 1);
        Ok(())
    }

    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        // TODO: Forward to slave?
        Ok(())
    }
}
