mod arch_x86_64;

// #[cfg(target_arch = "x86_64")]
pub use self::arch_x86_64::{Context, pause, jump_to_userspace};

