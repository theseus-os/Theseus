#![no_std]
#![feature(rustc_private)]


#[macro_use] extern crate cfg_if;

cfg_if!{
if #[cfg(target_arch="x86_64")] {

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;
extern crate xmas_elf;
extern crate memory;
extern crate memory_initialization;
extern crate kernel_config;
extern crate util;
extern crate crate_name_utils;
extern crate crate_metadata;
extern crate rustc_demangle;
extern crate cow_arc;
extern crate qp_trie;
extern crate root;
extern crate vfs_node;
extern crate fs_node;
extern crate path;
extern crate memfs;
extern crate cstr_core;
extern crate hashbrown;

mod arch_x86_64;
pub use arch_x86_64::*;

}
else if #[cfg(target_arch="arm")] {

mod arch_armv7em;
pub use arch_armv7em::*;

}
}
