#![no_std]

extern crate cfg_if;

cfg_if::cfg_if!{

if #[cfg(target_arch="x86_64")] {
    pub mod x86_64;
    pub use x86_64::memory;
    pub use x86_64::time;
    pub use x86_64::display;
} else if #[cfg(target_arch="arm")] {
    pub mod armv7em;
    pub use armv7em::memory;
}

}
