#![no_std]

extern crate log;
#[macro_use] extern crate cfg_if;

cfg_if!{

if #[cfg(target_arch="x86_64")] {

extern crate spin;
extern crate serial_port;

mod x86_64;
pub use x86_64::*;

} else if #[cfg(target_arch="arm")] {

extern crate cortex_m_semihosting;

mod armv7em;
pub use armv7em::*;

}

}
