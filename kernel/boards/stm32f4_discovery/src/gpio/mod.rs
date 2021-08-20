//! Implements device specific functionality relating to GPIO
#![no_std]

use irq_safety::MutexIrqSafe;
use core::cell::RefCell;

/// Exposes the board's GPIOA
static BOARD_GPIOA: MutexIrqSafe<RefCell<Option<stm32f407::GPIOA>>> = MutexIrqSafe::new(RefCell::new(None));
