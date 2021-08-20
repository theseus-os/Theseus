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
    use irq_safety::MutexIrqSafe;
    use core::cell::RefCell;

    // Exposed individual peripherals for use within the crate's submodules
    static BOARD_GPIOA: MutexIrqSafe<RefCell<Option<stm32f407::GPIOA>>> = MutexIrqSafe::new(RefCell::new(None));
    static BOARD_RCC: MutexIrqSafe<RefCell<Option<stm32f407::RCC>>> = MutexIrqSafe::new(RefCell::new(None));
    static BOARD_USART2: MutexIrqSafe<RefCell<Option<stm32f407::USART2>>> = MutexIrqSafe::new(RefCell::new(None));


    /// Initializes device peripherals for use.
    /// TODO: As we add support for more peripherals,
    /// we can figure out how to initialize them together.
    /// For now however, we initialize te devices individually as needed.
    pub fn init_peripherals () {
        let p = stm32f407::Peripherals::take().unwrap();
        BOARD_GPIOA.lock().replace(Some(p.GPIOA));
        BOARD_RCC.lock().replace(Some(p.RCC));
        BOARD_USART2.lock().replace(Some(p.USART2));
    }

    pub mod uart;
}
}