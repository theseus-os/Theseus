//! Structs for line discipline functionality.
//!
//! Line discipline functionality is split into two structs: `MasterDiscipline`
//! and `SlaveDiscipline`. This is done so that buffering can be done on the
//! slave side, which allows applications (using the slave) to toggle line
//! buffering without modifying the master.

use crate::{Reader, Writer};
use alloc::vec::Vec;
use core2::io::{Read, Write};

/// A line discipline used by the master.
///
/// Performs the following actions:
/// - Handles line feed characters
pub(crate) struct MasterDiscipline {
    pub(crate) master: Writer,
    pub(crate) slave: Writer,
}

impl Write for MasterDiscipline {
    fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize> {
        let mut master_buf = Vec::with_capacity(buf.len());
        let mut slave_buf = Vec::with_capacity(buf.len());

        // TODO: We could keep track of indexes where a carriage return occurs to avoid
        // allocating two extra vecs.

        for c in buf {
            if char::from(*c) == '\r' {
                master_buf.extend(b"\r\n");
                slave_buf.push(b'\n');
            } else {
                master_buf.push(*c);
                slave_buf.push(*c);
            }
        }

        self.master.lock().write(&master_buf)?;
        self.slave.lock().write(&slave_buf)?;

        Ok(buf.len())
    }

    fn flush(&mut self) -> core2::io::Result<()> {
        Ok(())
    }
}

/// A line discipline used by the slave.
///
/// Performs the following:
/// - Buffers input till a line feed character
/// - Executes special characters on the internal buffer (e.g. backspace)
pub(crate) struct SlaveDiscipline {
    reader: Reader,
    buf: Vec<u8>,
}

impl SlaveDiscipline {
    pub(crate) fn new(reader: Reader) -> Self {
        Self { reader, buf: Vec::new() }
    }
}

impl Read for SlaveDiscipline {
    fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize> {
        self.reader.lock().read_to_end(&mut self.buf)?;
        // TODO: We could cache whether or not a self.buf contains a newline to avoid
        // searching every time.
        if let Some(pos) = self.buf.iter().rposition(|c| *c == b'\n') {
            let read_len = core::cmp::min(buf.len(), pos + 1);
            // Drain 0..read_len of self.buf into buf
            buf[0..read_len].clone_from_slice(self.buf.drain(0..read_len).as_slice());
            Ok(read_len)
        } else {
            Ok(0)
        }
    }
}
