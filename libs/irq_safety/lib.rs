#![no_std]

#[macro_use] extern crate cfg_if;

cfg_if!{
    // Architecture dependent code for x86_64.
if #[cfg(target_arch="x86_64")] {

extern crate irq_safety;

mod arch_x86_64;
pub use arch_x86_64::*;

}

else if #[cfg(target_arch="arm")] {

extern crate spin;
extern crate cortex_m;
extern crate owning_ref;
extern crate stable_deref_trait;

mod arch_armv7em;
pub use arch_armv7em::*;

}
}
