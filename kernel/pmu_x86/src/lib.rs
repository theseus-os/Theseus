//! Support for Performance Monitoring Unit readouts.

#![no_std]
#![feature(integer_atomics)]
#![feature(asm)]
#![feature(alloc)]

extern crate spin;
#[macro_use] extern crate lazy_static;
extern crate x86_64;
extern crate raw_cpuid;
extern crate atomic_linked_list;
extern crate task;
extern crate memory;
extern crate irq_safety;
extern crate alloc;
#[macro_use] extern crate log;

use x86_64::registers::msr::*;
use x86_64::VirtualAddress;
use raw_cpuid::*;
use spin::Once;
use atomic_linked_list::atomic_map::*;
use task::get_my_current_task;
use core::sync::atomic::{AtomicU32, Ordering};
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;

//the number of samples to be taken, each sample the Atomic SAMPLE_COUNT decrements by 1 and once it's 0, the performance counter causing the interrupts is stopped 
const NUM_SAMPLES: u32 = 500; 

pub static PMU_VERSION: Once<u16> = Once::new();
pub static SAMPLE_START_VALUE: AtomicU32 = AtomicU32::new(0);
pub static SAMPLE_EVENT_TYPE_MASK: AtomicU32 = AtomicU32::new(0);
pub static SAMPLE_COUNT:AtomicU32 = AtomicU32::new(NUM_SAMPLES);

static RDPMC_FFC0: u32 = 1 << 30;
static RDPMC_FFC1: u32 = (1 << 30) + 1;
static RDPMC_FFC2: u32 = (1 << 30) + 2;

static PMC_ENABLE: u64 = 0x01 << 22;
static INTERRUPT_ENABLE: u64 = 0x01 << 20;
static UNHALTED_CYCLE_MASK: u64 = (0x03 << 16) | (0x00 << 8) | 0x3C;
static INST_RETIRED_MASK: u64 = (0x03 << 16) | (0x00 << 8) | 0xC0;
static UNHALTED_REF_CYCLE_MASK: u64 = (0x03 << 16) | (0x01 << 8) | 0x3C;
static LLC_REF_MASK: u64 = (0x03 << 16) | (0x4F << 8) | 0x2E;
static LLC_MISS_MASK: u64 = (0x03 << 16) | (0x41 << 8) | 0x2E;
static BR_INST_RETIRED_MASK: u64 = (0x03 << 16) | (0x00 << 8) | 0xC4;
static BR_MISS_RETIRED_MASK: u64 = (0x03 << 16) | (0x00 << 8) | 0xC5;

lazy_static!{
    pub static ref IP_LIST: MutexIrqSafe<Vec<VirtualAddress>> = MutexIrqSafe::new(Vec::with_capacity(NUM_SAMPLES as usize));
    static ref PMC_LIST: [AtomicMap<i32, bool>; 4] = [AtomicMap::new(), AtomicMap::new(), AtomicMap::new(), AtomicMap::new()];
}
static NUM_PMC: u32 = 4;


/// Initialization function that retrieves the version ID number. Version ID of 0 means no 
/// performance monitoring is avaialable on the CPU (likely due to virtualization without hardware assistance).
pub fn init() {
    let cpuid = CpuId::new();
    if let Some(perf_mon_info) = cpuid.get_performance_monitoring_info() {
        PMU_VERSION.call_once(||perf_mon_info.version_id() as u16);
        if let Some(pmu_ver) = PMU_VERSION.try() {  
            if pmu_ver > &0 {
                unsafe{
                    //clear values in counters and their settings
                    wrmsr(IA32_PERF_GLOBAL_CTRL, 0);
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
            if let Some(core_available) = PMC_LIST[self.msr_mask as usize].get(&self.core) {
                if *core_available {
                    return Err("Counter used for this event was marked as free, value stored is likely inaccurute.");
                } 
            } else {
                return Err("Counter used for this event was never claimed, value stored is likely inaccurute.");
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
           if let Some(core_available) = PMC_LIST[self.msr_mask as usize].get(&self.core) {
                if *core_available {
                    return Err("Counter used for this event was marked as free, value stored is likely inaccurute.");
                } 
            } else {
                return Err("Counter used for this event was never claimed, value stored is likely inaccurute.");
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
            // Counter 0 is used for sampling. Currently just skipping that counter for regular counting and using the other three.
            if i != 0 {
                //If counter i on this core has been used before (it's present in the AtomicMap) and is currently in use (it points to false), checks the next counter
                if let Some(core_available) = pmc.get(&my_core) {
                    if !(core_available) {
                        continue;
                    }
                }
                //Claims the counter using the AtomicMap and writes the values to initialize counter (except for enable bit)
                pmc.insert(my_core, false);
                unsafe{
                    wrmsr(IA32_PMC0 + (i as u32), 0);
                    wrmsr(IA32_PERFEVTSEL0 + (i as u32), event_mask);
                }
                return Ok(Counter{start_count: 0, msr_mask: i as u32, pmc: i as i32, core: my_core});
            }
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
    } else {
        return Err("Task structure not yet started. Must be initialized before counting events.");
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
    return Ok(Counter{start_count: count, msr_mask: msr_mask, pmc: -1, core: my_core});
}

/// It's important to do the rdpmc as quickly as possible when it's called. 
/// Calling get_my_current_task() and doing the calculations to unpack the result adds cycles, so I created a version where the core can be passed down.
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

/// Function to start interrupt process in order to take samples using PMU. 
pub fn start_samples(event_type: &'static str, event_per_sample: u32) {    
    let start_value = 0xffffffff - event_per_sample;
    
    //TODO: some sort of check to make sure a valid event_type
    let event_mask = match event_type {
        "INST_RETIRED.ANY" => UNHALTED_CYCLE_MASK,
        "CPU_CLK_UNHALTED.THREAD" => INST_RETIRED_MASK,
        "CPU_CLK_UNHALTED.REF" => UNHALTED_REF_CYCLE_MASK,
        "LONGEST_LAT_CACHE.REFERENCE" => LLC_REF_MASK,
        "LONGEST_LAT_CACHE.MISS" => LLC_MISS_MASK,
        "BR_INST_RETIRED.ALL_BRANCHES" => BR_INST_RETIRED_MASK,
        "BR_MISP_RETIRED.ALL_BRANCHES" => BR_MISS_RETIRED_MASK,	
        _ => UNHALTED_CYCLE_MASK,
    } | PMC_ENABLE | INTERRUPT_ENABLE;
    SAMPLE_START_VALUE.store(start_value, Ordering::SeqCst);
    SAMPLE_EVENT_TYPE_MASK.store(event_mask as u32, Ordering::SeqCst);
    unsafe{
    wrmsr(IA32_PMC0, start_value as u64);
    wrmsr(IA32_PERFEVTSEL0, event_mask | PMC_ENABLE | INTERRUPT_ENABLE);
    }
}

/// Function to stop the sampling interrupts. 
pub fn stop_samples() {
    unsafe{
        wrmsr(IA32_PERFEVTSEL0, 0);
        wrmsr(IA32_PMC0, 0);
        SAMPLE_EVENT_TYPE_MASK.store(0, Ordering::SeqCst);
        SAMPLE_START_VALUE.store(0, Ordering::SeqCst);

    }
}

/// Read 64 bit PMC (performance monitor counter).
pub fn rdpmc(msr: u32) -> u64 {
    let (high, low): (u32, u32);
    unsafe {
        asm!("rdpmc": "={eax}" (low), "={edx}" (high): "{ecx}" (msr) : "memory" : "volatile");
    }
    ((high as u64) << 32) | (low as u64)
}