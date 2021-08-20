//! Implements platform specific functionalities for the STM32F4_Discovery board,
//! exposing its peripherals and providing ways of interacting with them.
#![no_std]
#[macro_use] extern crate cfg_if;

cfg_if!{
if #[cfg(target_vendor = "stm32f407")] {
    extern crate stm32f4;
    extern crate spin;
    extern crate irq_safety;
    extern crate cortex_m;

    use stm32f4::stm32f407;
    use irq_safety::MutexIrqSafe;

    pub mod gpio;
    pub mod rcc;
    pub mod uart;

    /// Initializes device peripherals for use.
    /// TODO: As we add support for more peripherals,
    /// we can figure out how to initialize them together.
    /// For now however, we initialize te devices individually as needed.
    pub fn init_peripherals () {
        let p = stm32f407::Peripherals::take().unwrap();

        // Destructing p to get peripherals we need to initialize
        let stm32f407::Peripherals {
            GPIOA: gpioa,
            RCC: rcc,
            USART2: usart2,
            ..
        } = p;

        // Initializing the static variables associated with each peripheral
        gpio::BOARD_GPIOA.call_once(|| MutexIrqSafe::new(gpioa));
        rcc::BOARD_RCC.call_once(|| MutexIrqSafe::new(rcc));
        uart::BOARD_USART2.call_once(|| MutexIrqSafe::new(usart2));
    }

}
}