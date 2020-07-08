#![no_std]
#![feature(asm)]

#[macro_use] extern crate alloc;
extern crate x86_64;
extern crate apic;
extern crate spawn;

use alloc::vec::Vec;
use x86_64::registers::msr::*;

//address of the base MSR for L3 CAT usage
const IA32_L3_CBM_BASE: u32 = 0xc90u32;

#[derive(Clone, Copy, Debug)]
pub struct ClosDescriptor{
    pub closid: u8,
    pub bitmask: u32,
}

#[derive(Clone)]
pub struct ClosList{
    pub descriptor_list: Vec<ClosDescriptor>,
}

// function for finding the maximum supported clos id
// for more information, see page 2-48 vol. 4 of the Intel 64 and IA-32 Architectures Software Development manual
pub unsafe fn get_max_closid() -> u16 {
    let ret_32_bits : u32;
    asm!("cpuid"
	 : "={dx}"(ret_32_bits)
	 : "{ax}"(10u32), "{cx}"(2u32)
    );
    let ret : u16= (ret_32_bits & 0xffff) as u16;
    ret
}

// function that checks whether an 11 bit integer contains any nonconsecutive ones
fn only_consecutive_bits(mask : u32) -> bool{
    let mut reached_a_one = false;
    let mut reached_last_one = false;

    for i in 0..11{
	// checking whether we have found a zero after a one, in which case we have found the location of the last one
	if reached_a_one && (mask & (1 << i)  == 0){
	    reached_last_one = true;
	}
	// checking whether the current bit is a one
	reached_a_one = (mask & (1 << i)) > 0;
	if reached_a_one && reached_last_one{
	    // we have found a nonconsecutive one
	    return false;
	}
    }
    
    true
}

// a valid bitmask for CAT must be less than 0x7ff and cannot be 0; also, it may not have any non-consecutive ones
pub fn valid_bitmask(mask: u32) -> bool{
    // checking that the value is not greater than the maximum value of 0x7ff
    if mask > 0x7ff {
	return false;
    }
    // all 0's is also in invalid bitmask
    else if mask == 0{
	return false;
    }
    // finally, there are not allowed to be any nonconsecutive one-bits
    else if !only_consecutive_bits(mask){
	return false;
    }
    true
}

// function that will overwrite a single MSR for the CLOS described by CLOSDescriptor
pub fn update_clos(clos: ClosDescriptor) -> Result<(), &'static str>{
    if !valid_bitmask(clos.bitmask){
	return Err("Invalid bitmask passed to CAT.");
    }

    if clos.closid > 127{
	return Err("Closid must be less than 128.");
    }

    // setting the address of the msr that we need to write and writing our bitmask to the proper register
    let msr : u32 = IA32_L3_CBM_BASE + clos.closid as u32;
    unsafe{
	asm!("wrmsr"
		  :
		  : "{cx}"(msr), "{dx}"(0), "{ax}"(clos.bitmask)
	);
    }
    Ok(())
}

pub fn set_clos_on_single_core(clos_list: ClosList) -> Result<(), &'static str>{
    for clos in clos_list.descriptor_list{
	update_clos(clos)?;
    }
    Ok(())
}

pub fn set_clos(clos_list: ClosList) -> Result<(), &'static str>{
    let cores = apic::core_count();
    let mut tasks = Vec::with_capacity(cores);

    for i in 0..cores {
        let taskref = spawn::new_task_builder(set_clos_on_single_core, clos_list.clone())
            .name(format!("set_clos_on_core_{}", i))
	    .pin_on_core(i as u8)
            .spawn()?;
        tasks.push(taskref);
    }
    for t in &tasks {
        t.join()?;
        let _ = t.take_exit_value();
    }
    Ok(())
}

// function that will validate whether the classes of service specified in a given ClosList are set to their proper value
pub fn validate_clos_on_single_core(clos_list: ClosList) -> Result<(), (u32, u32)>{
    for clos in clos_list.descriptor_list{
	let reg = IA32_L3_CBM_BASE + clos.closid as u32;
	let value : u32 = rdmsr(reg) as u32;
	if value as u32 != clos.bitmask{
	    return Err((clos.bitmask, value));
	}
    }
    Ok(())
}
