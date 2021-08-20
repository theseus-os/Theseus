//! Implements platform specific functionalities for the STM32F4_Discovery board,
//! exposing its peripherals and providing ways of interacting with them.
#![no_std]
#![feature(const_raw_ptr_to_usize_cast)]
#[macro_use] extern crate cfg_if;

cfg_if!{
if #[cfg(target_vendor = "stm32f407")] {
    extern crate stm32f4;
    extern crate spin;
    extern crate irq_safety;
    extern crate cortex_m;

    use stm32f4::stm32f407;

    pub mod gpio;
    pub mod rcc;
    pub mod uart;

    /// Initializes device peripherals for use.
    /// TODO: As we add support for more peripherals,
    /// we can figure out how to initialize them together.
    /// For now however, we initialize te devices individually as needed.
    pub fn init_peripherals () {
        let p = stm32f407::Peripherals::take().unwrap();
        gpio::BOARD_GPIOA.lock().replace(Some(p.GPIOA));
        rcc::BOARD_RCC.lock().replace(Some(p.RCC));
        uart::BOARD_USART2.lock().replace(Some(p.USART2));
    }

}
}