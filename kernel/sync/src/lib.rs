#![no_std]

mod prevention;

pub mod mutex;

pub fn main(mutex: mutex::Mutex<prevention::IrqSafe, u8>) {
    *mutex.lock() = 3;
}
