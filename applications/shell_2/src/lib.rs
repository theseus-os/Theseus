#![feature(never_type)]
#![no_std]

use core::fmt::Write;
use embedded_hal::serial;
use noline::{builder::EditorBuilder, sync::embedded::IO as Io};
use tty::Slave;

pub fn temp(slave: Slave) {
    let mut discipline = slave.discipline();
    discipline.echo(false);
    discipline.canonical(false);
    drop(discipline);

    let wrapper = Wrapper(slave);
    let mut io = Io::new(wrapper);
    let mut editor = EditorBuilder::new_unbounded()
        .with_unbounded_history()
        .build_sync(&mut io)
        .unwrap();
    
    loop {
        editor.dedup_history();
        if let Ok(line) = editor.readline("> ", &mut io) {
            write!(io, "read: '{}'\n\r", line).unwrap();
        } else {
            write!(io, "failed to read line").unwrap();
        }
    }
}

struct Wrapper(Slave);

impl serial::Read<u8> for Wrapper {
    type Error = !;

    fn read(&mut self) -> nb::Result<u8, Self::Error> {
        Ok(self.0.read_byte())
    }
}

impl serial::Write<u8> for Wrapper {
    type Error = !;

    fn write(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
        self.0.write_byte(byte);
        Ok(())
    }

    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        // TODO: Forward to slave?
        Ok(())
    }
}
