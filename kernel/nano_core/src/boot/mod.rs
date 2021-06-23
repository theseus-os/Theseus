cfg_if!{

if #[cfg(target_arch="arm")] {

mod arch_armv7em;
pub use self::arch_armv7em::*;

}

}
