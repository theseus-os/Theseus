use async_channel::{Receiver, Sender};
use core2::io::Result;

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

    pub(crate) fn send_all<B>(&self, buf: B) -> Result<()>
    where
        B: AsRef<[u8]>,
    {
        self.sender.send_all(buf.as_ref()).map_err(|e| e.into())
    }

    pub(crate) fn receive(&self) -> Result<u8> {
        self.receiver.receive().map_err(|e| e.into())
    }

    pub(crate) fn receive_buf(&self, buf: &mut [u8]) -> Result<usize> {
        self.receiver.receive_buf(buf).map_err(|e| e.into())
    }

    pub(crate) fn try_receive_buf(&self, buf: &mut [u8]) -> Result<usize> {
        self.receiver.try_receive_buf(buf).map_err(|e| e.into())
    }
}
