#![no_std]
#![feature(stmt_expr_attributes)]

#[macro_use] extern crate cfg_if;

cfg_if!{

if #[cfg(target_arch="x86_64")] {

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate debugit;
extern crate irq_safety;
extern crate memory;
extern crate stack;
extern crate task;
extern crate runqueue;
extern crate scheduler;
extern crate mod_mgmt;
extern crate apic;
extern crate context_switch;
extern crate path;
extern crate fs_node;
extern crate catch_unwind;
extern crate fault_crate_swap;
extern crate pause;

mod arch_x86_64;
pub use arch_x86_64::*;

} else if #[cfg(target_arch="arm")] {

mod arch_armv7em;
pub use arch_armv7em::*;

}
}
