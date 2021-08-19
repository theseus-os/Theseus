//! Implements platform specific functionalities for the STM32F4_Discovery board,
//! exposing its peripherals and providing ways of interacting with them.
#![no_std]
#![feature(const_raw_ptr_to_usize_cast)]
#[macro_use] extern crate cfg_if;

cfg_if!{
if #[cfg(target_vendor = "stm32f407")] {
    extern crate stm32f4;
    extern crate spin;
    #[macro_use] extern crate irq_safety;
    extern crate lazy_static;

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

    /// Initializes device peripherals for use.
    /// TODO: As we add support for more peripherals,
    /// we can figure out how to initialize them together.
    /// For now however, we initialize te devices individually as needed.
    pub fn init_peripherals () {
        todo!("For now, peripherals are initialized individually as needed.");
    }
}
}