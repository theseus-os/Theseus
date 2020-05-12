//! A cfg-based wrapper that unifies rendezvous channels and async channels, for evaluation purposes.
//! 
//! The cfg option is `use_async_channel`, the default is to use the rendezvous channel.

#![no_std]
extern crate alloc;
extern crate async_channel;
extern crate rendezvous;

use alloc::string::String;


pub fn new_string_channel(_minimum_capacity: usize) -> (StringSender, StringReceiver) {
    #[cfg(use_async_channel)] {
        let (sender, receiver) = async_channel::new_channel::<String>(_minimum_capacity);
        return (StringSender { sender }, StringReceiver { receiver });
    }

    #[cfg(not(use_async_channel))] {
        let (sender, receiver) = rendezvous::new_channel::<String>();
        return (StringSender { sender }, StringReceiver { receiver });
    }
}

#[derive(Clone)]
pub struct StringSender {
    #[cfg(use_async_channel)]
    sender: async_channel::Sender<String>,
    #[cfg(not(use_async_channel))]
    sender: rendezvous::Sender<String>, 
}
impl StringSender {
    pub fn send(&self, msg: String) -> Result<(), &'static str> {
        #[cfg(use_async_channel)]
        return self.sender.send(msg).map_err("Sender returned error");

        #[cfg(not(use_async_channel))]
        self.sender.send(msg)
    }
}

#[derive(Clone)]
pub struct StringReceiver {
    #[cfg(use_async_channel)]
    receiver: async_channel::Receiver<String>,
    #[cfg(not(use_async_channel))]
    receiver: rendezvous::Receiver<String>, 
}
impl StringReceiver {
    pub fn receive(&self) -> Result<String, &'static str> {
        #[cfg(use_async_channel)]
        return self.receiver.receive().map_err("Receiver returned error");

        #[cfg(not(use_async_channel))]
        self.receiver.receive()
    }
}