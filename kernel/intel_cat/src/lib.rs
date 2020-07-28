//! Implements the basic functionality for controlling Intel CAT (Cache Allocation Technology).
//! Provides the ability to reserve exclusive or shared space in the LLC for a program or group of programs.

#![no_std]
#![feature(drain_filter)]


#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate irq_safety;
extern crate x86_64;
extern crate apic;
extern crate spawn;
extern crate task;
extern crate closid_settings;

use alloc::vec::*;
use x86_64::registers::msr::*;
use irq_safety::MutexIrqSafe;
#[cfg(use_intel_cat)]
use closid_settings::{ClosId, zero_closid, get_max_closid};

//address of the base MSR for L3 CAT usage
const IA32_L3_CBM_BASE: u32 = 0xc90u32;

// number of bits in a clos bitmask
const BITMASK_SIZE: u32 = 11;

#[cfg(use_intel_cat)]
/// Struct to represent the cache allocation belonging to a class of service.
#[derive(Clone, Copy, Debug)]
struct ClosDescriptor{
    /// Integer representing the class of service that this `ClosDescriptor` refers to. A `Task` whose `closid` field has the same value of `closid` will allocate to the cache region defined by `bitmask`.
    closid: ClosId,

    /// A string of bits representing the cache ways that this class of service has access to. If the i-th bit is a 1, tasks in this class of service may occupy that cache way; if it is 0, they may not occupy that cache way.
    bitmask: u32,

    /// If this value is true, the class of service occupies an exclusive region in the cache; if it is false, it occupies shared cache space with other classes of service.
    exclusive: bool,
}

#[cfg(use_intel_cat)]
impl ClosDescriptor{
    /// Function to remove the cache ways specified by the bitmask exclusive_space from the bit mask of the CLOS.
    /// This function is used within the `update_current_clos_list` function to remove the exclusive region from the allocations of all the other currently allocated classes of service.
    fn remove_exclusive_region(&mut self, exclusive_bitmask: u32){
	self.bitmask &= !exclusive_bitmask;
    }
}

#[cfg(use_intel_cat)]
/// List of `ClosDescriptor`s to describe the current state of existing cache allocations. 
#[derive(Clone, Debug)]
struct ClosList{
    /// Vector containing a `ClosDescriptor` for every currently active class of service.
    pub descriptor_list: Vec<ClosDescriptor>,

    /// Integer that describes the largest closid among the currently active classes of service.
    pub max_closid : u16,
}

#[cfg(use_intel_cat)]
impl ClosList{
    /// Create a new `ClosList` from a vector of `ClosDescriptor`s
    fn new(vals: Vec<ClosDescriptor>) -> Self{
	ClosList{
	    descriptor_list: vals,
	    max_closid: 0
	}
    }
}

#[cfg(use_intel_cat)]
// this vector of clos descriptors will contain information about the clos that have been allocated so far
lazy_static! {
    // `ClosList` that contains the current allocation of the cache
    static ref CURRENT_CLOS_LIST: MutexIrqSafe<ClosList> = MutexIrqSafe::new(
	ClosList::new(
	    vec![
		ClosDescriptor{
		    closid: zero_closid(),
		    bitmask: 0x7ff,
		    exclusive: false,
		}
	    ]
	)
    );
}

#[cfg(use_intel_cat)]
/// this function will return a new closid that is not in use
/// will return an error if the value of CURRENT_MAX_CLOSID is greater than or equal to the maximum closid on the system
fn get_free_closid() -> Result<u16, &'static str>{
    let current_max = CURRENT_CLOS_LIST.lock().max_closid;
    #[cfg(use_intel_cat)]{
        if current_max >= get_max_closid(){
        return Err("Could not create new clos, no free closids are available.");
        }
    }
    Ok(current_max + 1)
}

#[cfg(use_intel_cat)]
/// this function will increment the current value of CURRENT_MAX_CLOSID
fn increment_max_closid(){
    CURRENT_CLOS_LIST.lock().max_closid += 1;
}

#[cfg(use_intel_cat)]
/// this function will update the clos_descriptor entry corresponding to the closid of clos into CURRENT_CLOS_LIST
/// if the closid has already been allocated, the corresponding entry in CURRENT_CLOS_LIST will be updated with the proper clos
/// otherwise clos will be appended onto the list
fn update_current_clos_list(clos: ClosDescriptor){
    let mut current_list = CURRENT_CLOS_LIST.lock();
    // if the clos id in the clos descriptor is already in the current clos list we remove it
    current_list.descriptor_list.drain_filter(|x| x.closid.0 == clos.closid.0);
    // if clos is meant to have exclusive cache access, we will remove this region from all the other clos descriptors
    if clos.exclusive{
	for i in 1..current_list.descriptor_list.len(){
	    current_list.descriptor_list[i].remove_exclusive_region(clos.bitmask);
	}
    }
    current_list.descriptor_list.push(clos);
}

#[cfg(use_intel_cat)]
/// function that checks whether an BITMASK_SIZE bit integer contains any nonconsecutive ones
fn only_consecutive_bits(mask : u32) -> bool{
    let mut reached_a_one = false;
    let mut reached_last_one = false;

    for i in 0..BITMASK_SIZE{
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

#[cfg(use_intel_cat)]
/// this function will return a bitmask representing all the space that is currently not allocated exclusively in the cache
fn get_current_free_cache_space() -> u32 {
    // we will loop through the current cache allocation list and mark off all the regions that are allocated exclusively
    let exclusive_region: u32 = CURRENT_CLOS_LIST.lock().descriptor_list.iter().fold(0, |acc, x| if x.exclusive {acc | x.bitmask} else {acc});
    // 0x7ff is the bitmask representing the entire space in the cache, so the formula below will yield the bitmask containing all the cache ways that are not exclusively allocated
    0x7ff & (!exclusive_region)
}

#[cfg(use_intel_cat)]
/// attempt to create a `ClosDescriptor` to describe an allocation of n megabytes of exclusive cache space
/// if the requested space is not available or no new closids are available, then return an error
fn create_exclusive_clos_descriptor(size_in_megabytes: u32) -> Result<ClosDescriptor, &'static str>{
    //check whether n is between 1 and BITMASK_SIZE megabytes, exclusively
    if size_in_megabytes < 1 {
	return Err("Cache allocations must contain at least one MB of cache space.");
    }
    else if size_in_megabytes > (BITMASK_SIZE - 1) {
	return Err("You may only request up to (BITMASK_SIZE - 1) MB of cache space.");
    }

    // generate a free closid
    let new_closid = get_free_closid()?;
    let free_region = get_current_free_cache_space();

    // we will try to reserve n consecutive bits in a bitmask, if this is not possible, we will return an error
    let mut attempt_bitmask = (1 << size_in_megabytes) - 1;

    // try successive consecutive regions of the cache to see if there is a valid free region of n consecutive MBs
    while attempt_bitmask < (1 << (BITMASK_SIZE - 1)){
	if (attempt_bitmask & free_region) == attempt_bitmask{
	    return Ok(
		ClosDescriptor{
		    closid: ClosId::new(new_closid)?,
		    bitmask: attempt_bitmask,
		    exclusive: true,
		}
	    );
	}

	attempt_bitmask = attempt_bitmask << 1;
    }

    // there were no valid consecutive regions
    Err("Could not reserve the requested amount of cache space.")
}

#[cfg(use_intel_cat)]
/// attempt to create a `ClosDescriptor` to describe an allocation of n megabytes of nonexclusive cache space
/// if the requested space is not available or no new closids are available, then return an error
fn create_nonexclusive_clos_descriptor(size_in_megabytes: u32) -> Result<ClosDescriptor, &'static str>{
    //check whether n is between 1 and (BITMASK_SIZE - 1) megabytes, exclusively
    if size_in_megabytes < 1 {
	return Err("Cache allocations must contain at least one MB of cache space.");
    }
    else if size_in_megabytes > BITMASK_SIZE {
	return Err("You may only request up to BITMASK_SIZE MB of cache space.");
    }

    // generate a free closid
    let new_closid = get_free_closid()?;
    let free_region = get_current_free_cache_space();

    // we will try to reserve the last  n consecutive bits in a bitmask, if this is not possible, we will return an error
    let attempt_bitmask = ((1 << size_in_megabytes) - 1) << (BITMASK_SIZE - size_in_megabytes);

    if (attempt_bitmask & free_region) == attempt_bitmask{
        let closid_temp = ClosId::new(new_closid)?;
	    // the last n cache-ways are free to use, so we will return a `Clos Descriptor` representing this region
	    return Ok(
            ClosDescriptor{
            closid: closid_temp,
            bitmask: attempt_bitmask,
            exclusive: false,
            }
	    );
    }
    
    // the last n cache-ways were not free, so we will return an error
    Err("Could not reserve the requested amount of cache space.")
}

#[cfg(use_intel_cat)]
/// a valid bitmask for CAT must be less than 0x7ff and cannot be 0; also, it may not have any non-consecutive ones
fn valid_bitmask(mask: u32) -> bool{
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

#[cfg(use_intel_cat)]
/// function that will overwrite a single MSR for the CLOS described by CLOSDescriptor
fn update_clos(clos: ClosDescriptor) -> Result<(), &'static str>{
    if !valid_bitmask(clos.bitmask){
	return Err("Invalid bitmask passed to CAT.");
    }
    
	
    // setting the address of the msr that we need to write and writing our bitmask to the proper register
    let msr : u32 = IA32_L3_CBM_BASE + clos.closid.0 as u32;
    unsafe { wrmsr(msr, clos.bitmask as u64); }
    Ok(())
}

#[cfg(use_intel_cat)]
/// sets the MSRs on a single CPU core to the values described in `clos_list`
fn set_clos_on_single_core(clos_list: ClosList) -> Result<(), &'static str>{
    for clos in clos_list.descriptor_list{
	update_clos(clos)?;
    }
    Ok(())
}

#[cfg(use_intel_cat)]
/// calls `set_clos_on_single_core` on all available CPU cores
fn set_clos() -> Result<(), &'static str>{
    let cores = apic::core_count();
    let mut tasks = Vec::with_capacity(cores);

    for i in 0..cores {
        let taskref = spawn::new_task_builder(set_clos_on_single_core, CURRENT_CLOS_LIST.lock().clone())
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

#[cfg(use_intel_cat)]
/// This function is intended to reset the current cache allocations to their default state, i.e. set all Classes of Service to occupy the whole LLC.
/// After this, the value of `CURRENT_CLOS_LIST` will be reset to its default value.
pub fn reset_cache_allocations() -> Result<(), &'static str>{
    // Creating a temporary `ClosList` to be used to reset all the bitmasks to 0x7ff
    let mut temp_vec : Vec<ClosDescriptor> = Vec::new();
    for i in 0..CURRENT_CLOS_LIST.lock().descriptor_list.len(){
    let closid_temp = ClosId::new(i as u16)?;
	temp_vec.push(
	    ClosDescriptor{
		closid: closid_temp,
		bitmask: 0x7ff,
		exclusive: false,
	    }
	);
    }
    // setting CURRENT_CLOS_LIST to be the temporary list and then resetting MSRs
    CURRENT_CLOS_LIST.lock().descriptor_list = temp_vec.clone();
    set_clos()?;

    // resetting CURRENT_CLOS_LIST to its default value
    *CURRENT_CLOS_LIST.lock() = ClosList::new(
	vec![
	    ClosDescriptor{
		closid: zero_closid(),
		bitmask: 0x7ff,
		exclusive: false,
	    }
	]
    );

    // setting all tasks to the default closid of 0
    #[cfg(use_intel_cat)]
    for (id, taskref) in task::TASKLIST.lock().iter() {
        taskref.set_closid(0)?;
    }

    // setting all tasks to the default closid of 0
    #[cfg(use_intel_cat)]
    for (id, taskref) in task::TASKLIST.lock().iter() {
        taskref.set_closid(0)?;
    }
    
    Ok(())
}

#[cfg(use_intel_cat)]
/// Function that will add a class of service with either an exclusive or non-exclusive cache region of size n megabytes.
/// The return value is the closid of the new class of service if successful, otherwise an error will be returned.
pub fn allocate_clos(size_in_megabytes : u32, exclusive: bool) -> Result<ClosId, &'static str>{
    // attempting to create a new `ClosDescriptor` with the requested amount of space
    let allocated_clos = match exclusive{
        true => create_exclusive_clos_descriptor(size_in_megabytes)?,
        false => create_nonexclusive_clos_descriptor(size_in_megabytes)?
    };

    // add the new clos to the current clos list and increment the max closid
    update_current_clos_list(allocated_clos);

    increment_max_closid();

    // update the current state of the MSRs and return the closid of the new clos if successful, otherwise pass on the error message
    match set_clos() {
	Ok(()) => Ok(allocated_clos.closid),
	Err(e) => Err(e)
    }
}

#[cfg(use_intel_cat)]
/// Sets the closid of the current `Task`
pub fn set_closid_on_current_task(new_closid: u16) -> Result<(), &'static str>{
    if let Some(taskref) = task::get_my_current_task() {
        taskref.set_closid(new_closid)?;
    }
    else{
        return Err("Could not find task struct.");
    }
    Ok(())
}

#[cfg(use_intel_cat)]
/// Function that will validate whether the classes of service specified in a given `ClosList` are set to their proper value.
/// Returns `Ok(())` upon success, otherwise the pair of values (expected value, value read from the msr). 
pub fn validate_clos_on_single_core() -> Result<(), (u32, u32)>{
    for clos in CURRENT_CLOS_LIST.lock().descriptor_list.clone(){
	let reg = IA32_L3_CBM_BASE + clos.closid.0 as u32;
	let value : u32 = rdmsr(reg) as u32;
	if value as u32 != clos.bitmask{
	    return Err((clos.bitmask, value));
	}
    }
    Ok(())
}

/*
#[cfg(use_intel_cat)]
/// Function that returns the current state of the `CURRENT_CLOS_LIST` variable.
pub fn get_current_cache_allocation() -> ClosList{
    CURRENT_CLOS_LIST.lock().clone()
}
*/
