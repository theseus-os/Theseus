#![no_std]
#![feature(panic_info_message)]

#[macro_use] extern crate cfg_if;
#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate stack;
extern crate kernel_config;
extern crate context_switch;
extern crate spin;
extern crate irq_safety;
extern crate environment;
extern crate mod_mgmt;
#[macro_use] extern crate lazy_static;

cfg_if!{
// Architecture dependent code for x86_64.
if #[cfg(target_arch="x86_64")] {

extern crate tss;
extern crate root;
extern crate x86_64;

mod arch_x86_64;
pub use arch_x86_64::*;

}

else if #[cfg(target_arch="arm")] {

mod arch_armv7em;
pub use arch_armv7em::*;

}
}
