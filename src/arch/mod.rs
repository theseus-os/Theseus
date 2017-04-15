mod arch_x86_64;

// #[cfg(target_arch = "x86_64")]  // TODO: can we have a cfg block { } ?
pub use self::arch_x86_64::Registers; 

// #[cfg(target_arch = "x86_64")] 
pub use self::arch_x86_64::ArchTaskState; 

// #[cfg(target_arch = "x86_64")] 
pub use self::arch_x86_64::pause;

