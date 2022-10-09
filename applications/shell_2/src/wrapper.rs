use embedded_hal::serial;
use tty::Slave;

pub(crate) struct Wrapper<'a>(pub(crate) &'a Slave);

impl<'a> serial::Read<u8> for Wrapper<'a> {
    type Error = core2::io::Error;

    fn read(&mut self) -> nb::Result<u8, Self::Error> {
        self.0.read_byte().map_err(|e| e.into())
    }
}

impl<'a> serial::Write<u8> for Wrapper<'a> {
    type Error = core2::io::Error;

    fn write(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
        self.0.write_byte(byte).map_err(|e| e.into())
    }

    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        // TODO: Forward to slave?
        Ok(())
    }
}
