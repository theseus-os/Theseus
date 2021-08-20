//! Implements board specific functionality relating to the RCC

use irq_safety::MutexIrqSafe;
use spin::Once;
use stm32f4::stm32f407;

/// Exposses the board's RCC
pub static BOARD_RCC: Once<MutexIrqSafe<stm32f407::RCC>> = Once::new();
