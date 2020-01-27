//! A cfg-based wrapper that unifies rendezvous channels and async channels, for evaluation purposes.
//! 
//! The cfg option is `use_async_channel`, the default is to use the rendezvous channel.

#![no_std]
#[macro_use] extern crate cfg_if;

cfg_if! {
    
// use async
if #[cfg(use_async_channel)] {
    extern crate async_channel;
    pub use async_channel::{Sender, Receiver};

    /// Creates a new channel based on the chosen `cfg(use_async_channel)`.
    /// If that config is given, an async channel will be created with the given `minimum_capacity`
    /// otherwise a zero-capacity rendezvous channel will be created (ignoring the given `minimum_capacity`). 
    /// 
    /// This is a convenience function that unifies the two channel types, for evaluation purposes.
    pub fn new_channel<T: Send>(minimum_capacity: usize) -> (Sender<T>, Receiver<T>) {
        async_channel::new_channel(minimum_capacity)
    }
} 

// use rendezvous
else {
    extern crate rendezvous;
    pub use rendezvous::{Sender, Receiver};

    /// Creates a new channel based on the chosen `cfg(use_async_channel)`.
    /// If that config is given, an async channel will be created with the given `minimum_capacity`
    /// otherwise a zero-capacity rendezvous channel will be created (ignoring the given `minimum_capacity`). 
    /// 
    /// This is a convenience function that unifies the two channel types, for evaluation purposes.
    pub fn new_channel<T: Send>(_minimum_capacity: usize) -> (Sender<T>, Receiver<T>) {
        rendezvous::new_channel()
    }
}
}
