#![no_std]
#[macro_use] extern crate cfg_if;

cfg_if!{
if #[cfg(target_vendor = "stm32f407")] {
    extern crate irq_safety;
    use stm32f4::stm32f407;
    use core::cell::RefCell;
    use cortex_m::interrupt::{self, Mutex};
    use lazy_static::lazy_static;
    use irq_safety::MutexIrqSafe;

    lazy_static!{
        pub static ref STM_PERIPHERALS : MutexIrqSafe<stm32f407::Peripherals> = {
            let p = stm32f407::Peripherals::take().unwrap();
            MutexIrqSafe::new(p)
        };
    }
}
}