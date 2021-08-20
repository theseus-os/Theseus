//! Implements board specific functionality relating to the RCC

use irq_safety::MutexIrqSafe;
use core::cell::RefCell;
use stm32f4::stm32f407;

/// Exposses the board's RCC
pub static BOARD_RCC: MutexIrqSafe<RefCell<Option<stm32f407::RCC>>> = MutexIrqSafe::new(RefCell::new(None));
