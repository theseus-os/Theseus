//! Implements device specific functionality relating to GPIO

use irq_safety::MutexIrqSafe;
use core::cell::RefCell;
use stm32f4::stm32f407;

/// Exposes the board's GPIOA
pub static BOARD_GPIOA: MutexIrqSafe<RefCell<Option<stm32f407::GPIOA>>> = MutexIrqSafe::new(RefCell::new(None));
