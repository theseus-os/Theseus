// TODO: Use more efficient data structure?
// TODO: A sender and receiver are the same under the hood. Hypothetically we
// could store just one.
// FIXME: async_channel is not a proper mpmc
// FIXME: Error handling

use async_channel::{Receiver, Sender};
use core2::io::{ErrorKind, Result};

#[derive(Clone)]
pub(crate) struct Channel {
    sender: Sender<u8>,
    receiver: Receiver<u8>,
}

impl Channel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = async_channel::new_channel(256);
        Self { sender, receiver }
    }

    pub(crate) fn send(&self, byte: u8) -> Result<()> {
        self.sender.send(byte).map_err(|e| e.into())
    }

    pub(crate) fn send_buf<B>(&self, buf: B) -> Result<()>
    where
        B: AsRef<[u8]>,
    {
        // TODO: Don't fail if we can't send entire buf.
        for byte in buf.as_ref() {
            self.send(*byte)?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn try_send(&self, byte: u8) -> Result<()> {
        self.sender.try_send(byte).map_err(|(_, e)| e.into())
    }

    pub(crate) fn receive(&self) -> Result<u8> {
        self.receiver.receive().map_err(|e| e.into())
    }

    pub(crate) fn receive_buf(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut byte = self.receive()?;
        let mut read = 0;

        loop {
            buf[read] = byte;
            read += 1;

            if read == buf.len() {
                return Ok(read);
            }

            byte = match self.try_receive() {
                Ok(b) => b,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(read),
                Err(e) => return Err(e),
            };
        }
    }

    pub(crate) fn try_receive(&self) -> Result<u8> {
        self.receiver.try_receive().map_err(|e| e.into())
    }

    pub(crate) fn try_receive_buf(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        buf[0] = self.try_receive()?;

        if buf.len() == 1 {
            return Ok(1);
        }

        for (idx, item) in buf.iter_mut().enumerate().skip(1) {
            *item = match self.try_receive() {
                Ok(byte) => byte,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(idx),
                Err(e) => return Err(e),
            };
        }

        Ok(buf.len())
    }
}
