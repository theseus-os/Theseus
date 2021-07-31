//! Implements platform specific functionalities for the STM32F4_Discovery board, exposing its peripherals and providing ways of interacting with them.
#![no_std]
#[macro_use] extern crate cfg_if;

cfg_if!{
if #[cfg(target_vendor = "stm32f407")] {
    use stm32f4::stm32f407;
    use core::cell::RefCell;
    use cortex_m::interrupt::{self, Mutex};
    use lazy_static::lazy_static;

    lazy_static!{
        /// This struct exposes device-specific peripherals conforming to the `rust2svd` API.
        /// In order to allow safe sharing, we must first wrap the `Peripherals` struct in a `RefCell` to add the `Sync` trait, then we can use `cortex_m::interrupt::Mutex` to allow peripherals to be locked.
        /// For more information on why we must wrap the `Peripherals` struct in a `RefCell` and a `Mutex`, see [here](https://docs.rust-embedded.org/book/concurrency/index.html)
        pub static ref STM_PERIPHERALS : Mutex<RefCell<stm32f407::Peripherals>> = {
            let p = stm32f407::Peripherals::take().unwrap();
            Mutex::new(RefCell::new(p))
        };
    }

    pub mod uart;

    /// Initializes device peripherals for use.
    pub fn init_peripherals () {
        uart::uart_init();
    }
}
}