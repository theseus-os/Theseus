// TODO: Use more efficient data structure?
// TODO: A sender and receiver are the same under the hood. Hypothetically we
// could store just one.
// FIXME: async_channel is not a proper mpmc
// FIXME: Error handling

use async_channel::{ChannelError, Receiver, Sender};

#[derive(Clone)]
pub(crate) struct Channel {
    sender: Sender<u8>,
    receiver: Receiver<u8>,
}

impl Channel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = async_channel::new_channel(1024);
        Self { sender, receiver }
    }

    pub(crate) fn send(&self, byte: u8) {
        self.sender.send(byte).unwrap();
    }

    pub(crate) fn send_buf<B>(&self, buf: B)
    where
        B: AsRef<[u8]>,
    {
        for byte in buf.as_ref() {
            self.send(*byte);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn try_send(&self, byte: u8) -> bool {
        match self.sender.try_send(byte) {
            Ok(_) => true,
            Err((_, ChannelError::ChannelFull)) => false,
            Err(_) => panic!(),
        }
    }

    pub(crate) fn receive(&self) -> u8 {
        self.receiver.receive().unwrap()
    }

    pub(crate) fn receive_buf(&self, buf: &mut [u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }

        let mut byte = self.receive();
        let mut read = 0;

        loop {
            read += 1;
            buf[read] = byte;

            if read == buf.len() {
                return read;
            }

            byte = match self.try_receive() {
                Some(b) => b,
                None => return read,
            };
        }
    }

    pub(crate) fn try_receive(&self) -> Option<u8> {
        match self.receiver.try_receive() {
            Ok(byte) => Some(byte),
            Err(ChannelError::ChannelEmpty) => None,
            Err(_) => panic!(),
        }
    }
}
