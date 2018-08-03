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
use core::sync::atomic::{AtomicU32, Ordering, AtomicUsize, AtomicBool};
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;

pub static PMU_VERSION: Once<u16> = Once::new();
pub static SAMPLE_START_VALUE: AtomicUsize = AtomicUsize::new(0);
pub static SAMPLE_TASK_ID: AtomicUsize = AtomicUsize::new(0);
pub static SAMPLE_COUNT:AtomicU32 = AtomicU32::new(0);

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
    pub static ref IP_LIST: MutexIrqSafe<Vec<VirtualAddress>> = MutexIrqSafe::new(Vec::with_capacity(SAMPLE_COUNT.load(Ordering::SeqCst) as usize));
    pub static ref TASK_ID_LIST: MutexIrqSafe<Vec<usize>> = MutexIrqSafe::new(Vec::with_capacity(SAMPLE_COUNT.load(Ordering::SeqCst) as usize));
    static ref PMC_LIST: [AtomicMap<i32, bool>; 4] = [AtomicMap::new(), AtomicMap::new(), AtomicMap::new(), AtomicMap::new()];
    static ref CORES_SAMPLING: [AtomicBool; 4] = [AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)];
    static ref RESULTS_READY: [AtomicBool; 4] = [AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)];
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

/// Used to select event type to count. Event types are described on page 18-4 of Intel SDM 3B September 2016 Edition.
/// https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3b-part-2-manual.pdf
pub enum EventType{
    InstructionsRetired,
    UnhaltedThreadCycles,
    UnhaltedReferenceCycles,
    LongestLatCacheRef,
    LongestLatCacheMiss,
    BranchInstructionsRetired,
    BranchMispredictRetired,
}


impl Counter {
    /// Creates a Counter object and assigns a physical counter for it. 
    /// If it's a PMC, writes the UMASK and Event Code to the relevant MSR, but leaves enable bit clear.  
    pub fn new(event: EventType) -> Result<Counter, &'static str> {
        // ensures PMU available and initialized
        check_pmu_availability()?;
        
        match event {
            EventType::InstructionsRetired => return safe_rdpmc(RDPMC_FFC0),
            EventType::UnhaltedThreadCycles => return safe_rdpmc(RDPMC_FFC1),
            EventType::UnhaltedReferenceCycles => return safe_rdpmc(RDPMC_FFC2),
            EventType::LongestLatCacheRef => return programmable_start(LLC_REF_MASK),
            EventType::LongestLatCacheMiss => return programmable_start(LLC_MISS_MASK),
            EventType::BranchInstructionsRetired => return programmable_start(BR_INST_RETIRED_MASK),
            EventType::BranchMispredictRetired => return programmable_start(BR_MISS_RETIRED_MASK),	
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
        
        // If the msr_mask indicates it's a programmable counter, clears event counting settings and counter 
        if self.msr_mask < NUM_PMC {
            unsafe{
                wrmsr(IA32_PERFEVTSEL0 + self.msr_mask as u32, 0);
                wrmsr(IA32_PMC0 + self.msr_mask as u32, 0);
            }
            
            // Sanity check to make sure the counter wasn't stopped and freed previously
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
            // Counter 0 is used for sampling.
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
    check_pmu_availability()?;

    let my_core;
    if let Some(my_task) = get_my_current_task() {
        my_core = my_task.write().running_on_cpu as i32;
    } else {
        return Err("Task structure not yet started. Must be initialized before counting events.");
    }

    let count = rdpmc(msr_mask);
    return Ok(Counter{start_count: count, msr_mask: msr_mask, pmc: -1, core: my_core});
}

/// It's important to do the rdpmc as quickly as possible when it's called. 
/// Calling get_my_current_task() and doing the calculations to unpack the result adds cycles, so I created a version where the core can be passed down.
pub fn safe_rdpmc_complete(msr_mask: u32, core: i32) -> Result<Counter, &'static str> {
    check_pmu_availability()?;
    
    let count = rdpmc(msr_mask);
    return Ok(Counter{start_count: count, msr_mask: msr_mask, pmc: -1, core: core});
}

/// Checks to ensure that PMU has been initialized and that if the PMU has been initialized, the Version ID shows that the system has performance monitoring capabilities. 
fn check_pmu_availability() -> Result<(), &'static str>  {
    if let Some(version) = PMU_VERSION.try() {
        if version < &1 {
            return Err("Version ID of 0: Performance monitoring not supported in this environment(likely either due to virtualization without hardware acceleration or old CPU).")
        }
    } else {
        return Err("PMU not yet initialized.")
    }

    Ok(())
}

/// Start interrupt process in order to take samples using PMU. IPs sampled are stored in IP_LIST. Task IDs that IPs came from are stored in ID_LIST.
/// event_type determines type of event that sampling interval is based on, event_per_sample is how many of those events should occur between each sample, 
/// task_id allows the user to choose to only sample from a specific task by inputting a number or sample from all tasks by inputting None, 
/// and sample_count specifies how many samples should be recorded.
pub fn start_samples(event_type: EventType, event_per_sample: u32, task_id: Option<usize>, sample_count: u32) -> Result<(),&'static str> {        
    if let Some(my_task) = get_my_current_task() {
        if CORES_SAMPLING[my_task.write().running_on_cpu as usize].swap(true, Ordering::SeqCst) {
            return Err("Sampling is already being performed on this CPU.")
        }
        if let Some(task_id_num) = task_id {
            SAMPLE_TASK_ID.store(task_id_num, Ordering::SeqCst);
        }
        SAMPLE_COUNT.store(sample_count, Ordering::SeqCst);
        if event_per_sample > core::u32::MAX || event_per_sample <= core::u32::MIN {
            return Err("Number of events per sample invalid: must be within unsigned 32 bit");
        }
        let start_value = core::u32::MAX - event_per_sample;
        IP_LIST.lock().clear();
        TASK_ID_LIST.lock().clear();
        check_pmu_availability()?;

        let event_mask = match event_type {
            EventType::InstructionsRetired => UNHALTED_CYCLE_MASK,
            EventType::UnhaltedThreadCycles => INST_RETIRED_MASK,
            EventType::UnhaltedReferenceCycles => UNHALTED_REF_CYCLE_MASK,
            EventType::LongestLatCacheRef => LLC_REF_MASK,
            EventType::LongestLatCacheMiss => LLC_MISS_MASK,
            EventType::BranchInstructionsRetired => BR_INST_RETIRED_MASK,
            EventType::BranchMispredictRetired => BR_MISS_RETIRED_MASK,	
        } | PMC_ENABLE | INTERRUPT_ENABLE;
        SAMPLE_START_VALUE.store(start_value as usize, Ordering::SeqCst);

        unsafe{
            wrmsr(IA32_PMC0, start_value as u64);
            wrmsr(IA32_PERFEVTSEL0, event_mask | PMC_ENABLE | INTERRUPT_ENABLE);
        }

        return Ok(());
    }
    Err("Task implentation not yet initialized.")
}

pub struct SampleResults {
    pub instruction_pointers: Vec<VirtualAddress>,
    pub task_ids:  Vec<usize>,
}

/// Function to manually stop the sampling interrupts. Returns a Result with an array which contains slices that hold instruction pointers and
/// task IDs. The first slice holds the 
pub fn stop_samples() -> Result<(), &'static str> {
    //immediately stops counting and clears the counter
    unsafe{
        wrmsr(IA32_PERFEVTSEL0, 0);
        wrmsr(IA32_PMC0, 0);
    }
    //clears values in atomics so that even if exception is somehow triggered, it stops at the next iteration
    SAMPLE_START_VALUE.store(0, Ordering::SeqCst);
    SAMPLE_TASK_ID.store(0, Ordering::SeqCst);
    if let Some(my_task) = get_my_current_task() {
        if CORES_SAMPLING[my_task.write().running_on_cpu as usize].load(Ordering::SeqCst) {
            RESULTS_READY[my_task.write().running_on_cpu as usize].store(true, Ordering::SeqCst);
            return Ok(());
        } else {
            return Err("Core wasn't marked as locked, sample results likely invalid.");
        }
    } else {
        return Err("Task structure not yet enabled.");
    }
}

/// Returns the samples that were stored during sampling in the form of a SampleResults object. 
/// If samples are not yet finished, forces them to stop.  
pub fn retrieve_samples() -> Result<SampleResults, &'static str> {
    if let Some(my_task) = get_my_current_task() {
        if !RESULTS_READY[my_task.write().running_on_cpu as usize].load(Ordering::SeqCst) {
            stop_samples()?;
        }  
        CORES_SAMPLING[my_task.write().running_on_cpu as usize].store(false, Ordering::SeqCst);
        RESULTS_READY[my_task.write().running_on_cpu as usize].store(false, Ordering::SeqCst);
        return Ok(SampleResults{instruction_pointers: IP_LIST.lock().clone(), task_ids: TASK_ID_LIST.lock().clone()});
        
    } 
    Err("Task implentation not yet initialized.")
}

/// Simple function to print values from SampleResults in a form that the script "post-mortem pmu analysis.py" can parse. 
pub fn print_samples(sample_results: &mut SampleResults) {
    while let Some(next_ip) = sample_results.instruction_pointers.pop() {
        if let Some(next_id) = sample_results.task_ids.pop() {
            debug!("{:x} {}", next_ip, next_id);
        }
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
