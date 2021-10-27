//! Implements device specific functionality relating to GPIO.

use irq_safety::MutexIrqSafe;
use spin::Once;
use stm32f4::stm32f407;

/// Exposes the board's GPIOA.
pub static BOARD_GPIOA: Once<MutexIrqSafe<stm32f407::GPIOA>> = Once::new();
