//! A cfg-based wrapper that unifies rendezvous channels and async channels, for evaluation purposes.
//! 
//! The cfg option is `use_sync_channel`, the default is to use the rendezvous channel.

#![no_std]
extern crate alloc;
extern crate sync_channel;
extern crate rendezvous;

use alloc::string::String;


pub fn new_string_channel(_minimum_capacity: usize) -> (StringSender, StringReceiver) {
    #[cfg(use_sync_channel)] {
        let (sender, receiver) = sync_channel::new_channel::<String>(_minimum_capacity);
        return (StringSender { sender }, StringReceiver { receiver });
    }

    #[cfg(not(use_sync_channel))] {
        let (sender, receiver) = rendezvous::new_channel::<String>();
        return (StringSender { sender }, StringReceiver { receiver });
    }
}

#[derive(Clone)]
pub struct StringSender {
    #[cfg(use_sync_channel)]
    sender: sync_channel::Sender<String>,
    #[cfg(not(use_sync_channel))]
    sender: rendezvous::Sender<String>, 
}
impl StringSender {
    #[cfg(use_sync_channel)]
    pub fn send(&self, msg: String) -> Result<(), &'static str> {
        self.sender.send(msg).map_err(|_e| "mpmc channel send error")
    }

    #[cfg(not(use_sync_channel))]
    pub fn send(&self, msg: String) -> Result<(), &'static str> {
        self.sender.send(msg)
    }
}

#[derive(Clone)]
pub struct StringReceiver {
    #[cfg(use_sync_channel)]
    receiver: sync_channel::Receiver<String>,
    #[cfg(not(use_sync_channel))]
    receiver: rendezvous::Receiver<String>, 
}
impl StringReceiver {
    #[cfg(use_sync_channel)]
    pub fn receive(&self) -> Result<String, &'static str> {
        self.receiver.receive().map_err(|_e| "mpmc channel receive error")
    }

    #[cfg(not(use_sync_channel))]
    pub fn receive(&self) -> Result<String, &'static str> {
        self.receiver.receive()
    }
}