//! Implements platform specific functionalities for the STM32F4_Discovery board, exposing its peripherals and providing ways of interacting with them.
#![no_std]
#[macro_use] extern crate cfg_if;

cfg_if!{
if #[cfg(target_vendor = "stm32f407")] {
    extern crate stm32f4;

    use stm32f4::stm32f407;
    use lazy_static::lazy_static;
    use irq_safety::MutexIrqSafe;

    lazy_static!{
        /// This struct exposes device-specific peripherals conforming to the `rust2svd` API.
        /// In order to allow safe sharing, we utilize `irq_safety::MutexIrqSafe`
        pub static ref STM_PERIPHERALS : MutexIrqSafe<stm32f407::Peripherals> = {
            let p = stm32f407::Peripherals::take().unwrap();
            MutexIrqSafe::new(p)
        };
    }

    pub mod uart;
    pub mod uart_pointers;

    /// Initializes device peripherals for use.
    pub fn init_peripherals () {
        uart_pointers::uart_init(uart_pointers::USART2_BASE);
    }
}
}