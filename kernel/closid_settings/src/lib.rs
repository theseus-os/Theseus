#![no_std]
#![feature(asm)]


extern crate spin;
#[macro_use] extern crate raw_cpuid;

use spin::Once;

#[cfg(use_intel_cat)]
#[derive(Clone, Copy, Debug)]
/// Struct that represents the closid of a class of service. This closid is used to identify a class of service for use with Intel CAT.
pub struct ClosId(pub u16);

#[cfg(use_intel_cat)]
// variables that will contain the maximum closid supported on the system, which will never change in a given hardware configuration, so it only needs to be calculated once
static MAX_CLOSID_INIT : Once<u16> = Once::new();

#[cfg(use_intel_cat)]
// for more information, see page 2-48 vol. 4 of the Intel 64 and IA-32 Architectures Software Development manual
fn get_max_closid_init() -> u16 {
    /*let ret_32_bits : u32;
    unsafe {
	asm!("cpuid"
	 : "={dx}"(ret_32_bits)
	 : "{ax}"(0x10u32), "{cx}"(0x1u32)
	);
}*/
    let result = raw_cpuid::cpuid!(0x10u32, 0x1u32);
    let ret : u16= (result.edx & 0xffff) as u16;
    ret
}

#[cfg(use_intel_cat)]
/// Function for finding the maximum supported clos id for use with Intel CAT (Cache Allocation Technology).
pub fn get_max_closid() -> u16 {
    *MAX_CLOSID_INIT.call_once(|| {
	get_max_closid_init()
    })
}

#[cfg(use_intel_cat)]
impl ClosId {
    /// Function to create a new `ClosId`. 
    /// The integer specified must be between 0 and the maximum supported closid on the system, inclusive.
    pub fn new(closid : u16) -> Result<ClosId, &'static str> {
        if closid > get_max_closid(){
            return Err("closid_settings : ERROR: Requested closid is greater than the maximum valid closid.");
        }
        Ok(ClosId(closid))
    }

    /// Sets the value of the IA32_PQR_ASSOC MSR to associate the current processor with the class of service identified by the `ClosId`.
    /// Called during the context switching routine.
    pub fn set_closid_on_processor(&self) {
        // update the IA32_PQR_ASSOC MSR to point to the new task's CLOS
        let IA32_PQR_ASSOC = 0xc8fu32;
        unsafe{
            asm!("wrmsr"
                :
                : "{cx}"(IA32_PQR_ASSOC), "{dx}"(self.0 as u32), "{ax}"(0)
            );
        }
    }
}

#[cfg(use_intel_cat)]
/// Returns a `ClosId` corresponding to clos 0. This will always be a valid closid, as the default closid is 0.
pub const fn zero() -> ClosId{
    ClosId(0)
}