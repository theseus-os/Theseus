//! Support for Performance Monitoring Unit readouts.

#![no_std]
#![feature(asm, naked_functions)]

extern crate spin;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate port_io;
extern crate x86_64;
extern crate pit_clock;
extern crate tsc;
extern crate raw_cpuid;
extern crate atomic_linked_list;
extern crate task;


use x86_64::registers::msr::*;
use x86_64::instructions::rdpmc;
use raw_cpuid::*;
use spin::Once;
use core::sync::atomic::{AtomicPtr, Ordering};
use atomic_linked_list::atomic_map::*;
use task::get_my_current_task;

pub static PMU_VERSION: Once<u16> = Once::new();


static RDPMC_FFC0: u32 = 1 << 30;
static RDPMC_FFC1: u32 = (1 << 30) + 1;
static RDPMC_FFC2: u32 = (1 << 30) + 2;

static PMC_ENABLE: u64 = 0x01 << 22;

static LLC_REF_MASK: u64 = (0x03 << 16) | (0x4F << 8) | 0x2E;
static LLC_MISS_MASK: u64 = (0x03 << 16) | (0x41 << 8) | 0x2E;
static BR_INST_RETIRED_MASK: u64 = (0x03 << 16) | (0x00 << 8) | 0xC4;
static BR_MISS_RETIRED_MASK: u64 = (0x03 << 16) | (0x00 << 8) | 0xC5;

//static `Once<AtomicMap<K,V>>`
lazy_static!{
    static ref PMC_LIST: [AtomicMap<i32, bool>; 4] = [AtomicMap::new(), AtomicMap::new(), AtomicMap::new(), AtomicMap::new()];
}
static NUM_PMC: u32 = 4;




/// Initialization function that right now only retrieves the version ID number. Version ID of 0 means no 
/// performance monitoring is avaialable on the CPU (likely due to virtualization without hardware assistance).
pub fn init() {
    let cpuid = CpuId::new();
    PMU_VERSION.call_once(|| cpuid.get_performance_monitoring_info().unwrap().version_id() as u16);
    debug!("IA32 PEBS Availability:  {}", rdmsr(IA32_MISC_ENABLE));
    

    if PMU_VERSION.try().unwrap() > &0 {
        unsafe{
            //clear values in counters and their settings
            wrmsr(MSR_PERF_GLOBAL_CTRL, 0);
            wrmsr(IA32_PMC0, 0);
            wrmsr(IA32_PMC1, 0);
            wrmsr(IA32_PMC2, 0);
            wrmsr(IA32_PMC3, 0);
            wrmsr(IA32_FIXED_CTR0, 0);
            wrmsr(IA32_FIXED_CTR1, 0);
            wrmsr(IA32_FIXED_CTR2, 0);
            //sets fixed function counters to count events at all privilege levels
            wrmsr(IA32_FIXED_CTR_CTRL, 0x333);
            //enables all counters: each counter has another enable bit in other MSRs so these should likely never be cleared once first set
            wrmsr(IA32_PERF_GLOBAL_CTRL, 0x07 << 32 | 0x0f);

        }
    }
}


pub struct Counter {
    pub start_count: u64,
    msr_mask: u32,
    pmc: i32,
    core: i32,
}


impl Counter {
    /// Creates a Counter object and assigns a physical counter for it. 
    /// If it's a PMC, writes the UMASK and Event Code to the relevant MSR, but leaves enable bit clear.  
    pub fn new(event: &'static str) -> Result<Counter, &'static str> {
        // ensures PMU available and initialized
        if let Some(version) = PMU_VERSION.try() {
            if version < &1 {
                return Err("Version ID of 0: Performance monitoring not supported in this environment(likely either due to virtualization without hardware acceleration or old CPU).")
            }
        } else {
            return Err("PMU not yet initialized.")
        }
        
        match event {
            "INST_RETIRED.ANY" => return safe_rdpmc(RDPMC_FFC0),
            "CPU_CLK_UNHALTED.THREAD" => return safe_rdpmc(RDPMC_FFC1),
            "CPU_CLK_UNHALTED.REF" => return safe_rdpmc(RDPMC_FFC2),
            "LONGEST_LAT_CACHE.REFERENCE" => return programmable_start(LLC_REF_MASK),
            "LONGEST_LAT_CACHE.MISS" => return programmable_start(LLC_MISS_MASK),
            "BR_INST_RETIRED.ALL_BRANCHES" => return programmable_start(BR_INST_RETIRED_MASK),
            "BR_MISP_RETIRED.ALL_BRANCHES" => return programmable_start(BR_MISS_RETIRED_MASK),	
            _ => return Err("Event either not supported or invalid, refer to https://download.01.org/perfmon/index/wsmex.html for event names."),
        }
    }
    
    /// Starts the count. Reads value fixed counter is at or, if it's a PMC, sets the enable bit. 
    pub fn start(&mut self) {
        if self.msr_mask > NUM_PMC {
            self.start_count = rdpmc(self.msr_mask);
        } else {
            self.start_count = 0;
            let umask = rdmsr(IA32_PERFEVTSEL0 + self.pmc as u32);
            unsafe{wrmsr(IA32_PERFEVTSEL0 + self.pmc as u32, umask | PMC_ENABLE);}
        }
    }
    
    /// Allows user to get count since start without stopping/releasing the counter.
    pub fn get_count_since_start(&self) -> Result<u64, &'static str> {
        //checks to make sure the counter hasn't already been released
        if self.msr_mask < NUM_PMC {
            if PMC_LIST[self.msr_mask as usize].get(&self.core).is_none(){
                return Err("Counter used for this event was never claimed, value stored is likely inaccurute.");
            } else if PMC_LIST[self.msr_mask as usize].get(&self.core).unwrap() == &true {
                return Err("Counter used for this event was marked as free, value stored is likely inaccurute.");
 
            }
        }
        
        Ok(rdpmc(self.msr_mask) - self.start_count)
    }
    
    /// Stops counting, releases the counter, and returns the count of events since the counter was initialized.
    pub fn end(&self) -> Result<u64, &'static str> {
        //unwrapping here might be an issue because it adds a step to event being counted, unsure how to get around
        let end_rdpmc = safe_rdpmc_complete(self.msr_mask, self.core)?;
        let end_val = end_rdpmc.start_count;
        
        //if the msr_mask indicates it's a programmable counter, clears event counting settings and counter 
        if self.msr_mask < NUM_PMC {
            unsafe{
                wrmsr(IA32_PERFEVTSEL0 + self.msr_mask as u32, 0);
                wrmsr(IA32_PMC0 + self.msr_mask as u32, 0);
            }
            
            //sanity check to make sure the counter wasn't stopped and freed previously
            if PMC_LIST[self.msr_mask as usize].get(&self.core).is_none(){
                return Err("Counter used for this event was never claimed, value stored is likely inaccurute.");
            } else if PMC_LIST[self.msr_mask as usize].get(&self.core).unwrap() == &true {
                return Err("Counter used for this event was marked as free, value stored is likely inaccurute.");
            }

            PMC_LIST[self.msr_mask as usize].insert(self.core, true);
            

        
        }
    return Ok(end_val - self.start_count);
    }

}


/// Does the work of iterating through programmable counters and using whichever one is free. Returns Err if none free
fn programmable_start(event_mask: u64) -> Result<Counter, &'static str> {
    
    let my_core: i32;
    
    if let Some(my_task) = get_my_current_task() {
        my_core = my_task.write().running_on_cpu as i32;
        for (i, pmc) in PMC_LIST.iter().enumerate() {
            if pmc.get(&my_core).is_some() {
                if !(pmc.get(&my_core).unwrap()) {
                    continue;
                }
            }
            pmc.insert(my_core, false);
            unsafe{
                    wrmsr(IA32_PERFEVTSEL0 + (i as u32), event_mask);
                    wrmsr(IA32_PMC0 + (i as u32), 0);
            }
            return Ok(Counter{start_count: 0, msr_mask: i as u32, pmc: i as i32, core: my_core});
        }
            return Err("All programmable counters currently in use.");

    }
    return Err("Task implentation not yet initialized.");

}

/// Calls the rdpmc function which is a wrapper for the x86 rdpmc instruction. This function ensures that performance monitoring is 
/// initialized and enabled.    
pub fn safe_rdpmc(msr_mask: u32) -> Result<Counter, &'static str> {
    let my_core;
    if let Some(my_task) = get_my_current_task() {
        my_core = my_task.write().running_on_cpu as i32;
    }
    // if performance monitoring has not been initialized or is unaivalable, returns an error, otherwise returns counter value
    // ensures PMU available and initialized
    if let Some(version) = PMU_VERSION.try() {
        if version < &1 {
            return Err("Version ID of 0: Performance monitoring not supported in this environment(likely either due to virtualization without hardware acceleration or old CPU).")
        }
    } else {
        return Err("PMU not yet initialized.")
    }
    
    let count = rdpmc(msr_mask);
    return Ok(Counter{start_count: count, msr_mask: msr_mask, pmc: -1, core: 0});
}

/// It's important to do the rdpmc as quickly as possible when it's called. 
/// Calling get_my_current_task() and doing the calculations to unpack the result adds cycles, so I created a version where the core can be passed down.
/// TODO: Is there some form of function overloading that might be helpful here?   
pub fn safe_rdpmc_complete(msr_mask: u32, core: i32) -> Result<Counter, &'static str> {
    // if performance monitoring has not been initialized or is unaivalable, returns an error, otherwise returns counter value
    // ensures PMU available and initialized
    if let Some(version) = PMU_VERSION.try() {
        if version < &1 {
            return Err("Version ID of 0: Performance monitoring not supported in this environment(likely either due to virtualization without hardware acceleration or old CPU).")
        }
    } else {
        return Err("PMU not yet initialized.")
    }
    
    let count = rdpmc(msr_mask);
    return Ok(Counter{start_count: count, msr_mask: msr_mask, pmc: -1, core: core});
}


/*
/// Returns the count of events that have happened since start_count from Counter was called. 
pub fn event_count(start_key: Result<Counter, &'static str>) -> Result<u64, &'static str> {
    //unwrapping here might be an issue because it adds a step to event being counted, unsure how to get around
    let counter = start_key?;
    let end_rdpmc = safe_rdpmc(counter.msr_mask)?;
    let end_val = end_rdpmc.start_count;
    
    //if the msr_mask indicates it's a programmable counter, clears event counting settings and counter 
    if counter.msr_mask < NUM_PMC {
        unsafe{
            wrmsr(IA32_PERFEVTSEL0 + counter.msr_mask as u32, 0);
            wrmsr(IA32_PMC0 + counter.msr_mask as u32, 0);
        }
        //sanity check to make sure the counter wasn't stopped and freed previously
        if PMC_LIST[counter.msr_mask as usize].swap(true, Ordering::SeqCst) {
            return Err("Counter used for this event was already marked as free, value stored is likely inaccurute.");
        }
        
    }
    
    return Ok(end_val - counter.start_count);
    
}

/// Initializes count for programmable counters, returns an integer which is used a 
pub fn start_count(event: &'static str) -> Result<Counter, &'static str> {
    match event {
        "INST_RETIRED.ANY" => return safe_rdpmc(RDPMC_FFC0),
        "CPU_CLK_UNHALTED.THREAD" => return safe_rdpmc(RDPMC_FFC1),
        "CPU_CLK_UNHALTED.REF" => return safe_rdpmc(RDPMC_FFC2),
        "LONGEST_LAT_CACHE.REFERENCE" => return programmable_start(LLC_REF_MASK),
        "LONGEST_LAT_CACHE.MISS" => return programmable_start(LLC_MISS_MASK),
        "BR_INST_RETIRED.ALL_BRANCHES" => return programmable_start(BR_INST_RETIRED_MASK),
        "BR_MISP_RETIRED.ALL_BRANCHES" => return programmable_start(BR_MISS_RETIRED_MASK),	
        _ => return Err("Event either not supported or invalid, refer to https://download.01.org/perfmon/index/wsmex.html for event names."),
    }
}

pub fn test_rdpmc() -> Result<u64, &'static str> {
    let tsc_val = tsc::get_tsc_frequency();
    Ok(rdmsr(0xc1))
    
}

pub fn print_unhalted_ref() -> Option<u64> {

    let og_ctrl_val: u64 = rdmsr(MSR_PERF_GLOBAL_CTRL);
    
    unsafe{
        //counters are disabled, set to 0, then re-enabled
        wrmsr(MSR_PERF_GLOBAL_CTRL, 0);
        
        wrmsr(0xc3, 0);
        wrmsr(0xc4, 0);
        wrmsr(MSR_PERF_FIXED_CTR0, 0);
        wrmsr(IA32_FIXED_CTR1, 0);
        wrmsr(IA32_FIXED_CTR2, 0);
        wrmsr(IA32_PERFEVTSEL1, 0x004101c2);
        wrmsr(IA32_PERFEVTSEL0, 0x01c1010e);
        wrmsr(IA32_PERFEVTSEL2, 0x01c1010e);
        wrmsr(IA32_PERFEVTSEL3, 0x004101a2);
        wrmsr(IA32_FIXED_CTR_CTRL, 0x1);
        wrmsr(IA32_PERF_GLOBAL_CTRL, 0x07 << 32 | 0x07);
        
        //wrmsr(IA32_PERFEVTSEL0, 0x01 << 22 | 0x03 << 16);
        //wrmsr(IA32_PERF_GLOBAL_CTRL, 0x0f << 32 | 0x0f)
    }
    let start = rdtsc();


    //pauses 1 thousandth of a second to allow time for counters to progress
    let _result = pit_clock::pit_wait(10000);
    let my_result = (start * 45) ^ 2;
    let c: u32 = (1<<30);
    


    unsafe{
        //counters again disbled
        wrmsr(IA32_PERF_GLOBAL_CTRL, 0);
        wrmsr(IA32_FIXED_CTR_CTRL, 0);
    }
    let end = rdtsc();
    let ret_val: u64 = rdmsr(IA32_FIXED_CTR1);
    //let (high, low): (u32, u32);
    
    
    
    //Some((cpuid_output as u64) >> 15)

    let cpuid = CpuId::new();

    debug!("It has PDCM (performance monitoring): {}", cpuid.get_feature_info().unwrap().has_pdcm());
    debug!("Version ID of performance monitoring: {}", cpuid.get_performance_monitoring_info().unwrap().version_id());
    debug!("Number of TSC ticks that passed in approximately same time that Fixed Ctr 0 had to run: {}", end - start);
    return Some(ret_val);

}
*/