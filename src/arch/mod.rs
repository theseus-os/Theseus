mod x86_64;

#[cfg(target_arch = "x86_64")]  // TODO: can we have a cfg block { } ?
pub use x86_64::Registers; 

#[cfg(target_arch = "x86_64")] 
pub use x86_64::ArchSpecificState; 

#[cfg(target_arch = "x86_64")] 
pub use x86_64::pause;

