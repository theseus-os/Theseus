//! A basic logger implementation for system-wide logging in Theseus. 
//!
//! This enables Theseus crates to use the `log` crate's macros anywhere.
//! Currently, log statements are written to one or more serial ports.

#![no_std]

extern crate log;
#[macro_use]
extern crate cfg_if;

cfg_if! {

if #[cfg(target_arch="x86_64")] {

extern crate spin;
extern crate serial_port;
extern crate irq_safety;

mod x86_64;
pub use x86_64::*;

} else if #[cfg(target_arch="arm")] {


cfg_if! {
    if #[cfg(target_vendor = "stm32f407")] {
    	extern crate uart;
    } else if #[cfg(target_vendor = "unknown")] {
        extern crate cortex_m_semihosting;
    }
}

mod armv7em;
pub use armv7em::*;

}

}
