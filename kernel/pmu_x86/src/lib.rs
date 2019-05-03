//! Support for Performance Monitoring Unit readouts.
//! 
//! # Example
//! ```
//! pmu_x86::init();
//! 
//! let counter_freq = 0xFFFFF;
//! let num_samples = 500;
//! let sampler = pmu_x86::start_samples(pmu_x86::EventType::UnhaltedReferenceCycles, counter_freq, None, num_samples);
//! 
//! if let Ok(my_sampler) = sampler {
//! 
//! 	// wait some time here
//! 	
//! 	if let Ok(mut samples) = pmu_x86::retrieve_samples() {
//! 		pmu_x86::print_samples(&mut samples);
//! 	}
//! }
//! ```
//!
//! # Note
//! Currently, the PMU-based sampler will only capture samples on the same core as it was initialized and started from. 
//! So, if you run `pmu_x86::init()` and `pmu_x86::start_samples()` on CPU core 2, it will only sample events on core 2.
//!

#![no_std]
#![feature(asm)]

extern crate spin;
#[macro_use] extern crate lazy_static;
extern crate x86_64;
extern crate raw_cpuid;
extern crate atomic_linked_list;
extern crate task;
extern crate memory;
extern crate irq_safety;
extern crate alloc;
extern crate apic;
#[macro_use] extern crate log;

use x86_64::registers::msr::*;
use x86_64::VirtualAddress;
use x86_64::structures::idt::ExceptionStackFrame;
use raw_cpuid::*;
use spin::{Once, Mutex};
use atomic_linked_list::atomic_map::*;
use core::sync::atomic::{AtomicU32, Ordering, AtomicUsize};
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::collections::BTreeSet;

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
    // Here 4 is the number of programmable counters
    static ref PMC_LIST: [AtomicMap<i32, bool>; 4] = [AtomicMap::new(), AtomicMap::new(), AtomicMap::new(), AtomicMap::new()];
    static ref CORES_SAMPLING: Mutex<BTreeSet<u8>> = Mutex::new(BTreeSet::new());
    static ref RESULTS_READY: Mutex<BTreeSet<u8>> = Mutex::new(BTreeSet::new());
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
    let my_core = apic::get_my_apic_id().ok_or("Couldn't get my apic id")? as i32;
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
            return Ok(Counter {
                start_count: 0, 
                msr_mask: i as u32, 
                pmc: i as i32, 
                core: my_core
            });
        }
    }
    return Err("All programmable counters currently in use.");
}

/// Calls the rdpmc function which is a wrapper for the x86 rdpmc instruction. This function ensures that performance monitoring is 
/// initialized and enabled.    
pub fn safe_rdpmc(msr_mask: u32) -> Result<Counter, &'static str> {
    check_pmu_availability()?;

    let my_core = apic::get_my_apic_id().ok_or("Couldn't get my apic id")? as i32;
    let count = rdpmc(msr_mask);
    return Ok(Counter {
        start_count: count, 
        msr_mask: msr_mask, 
        pmc: -1, 
        core: my_core
    });
}

/// It's important to do the rdpmc as quickly as possible when it's called. 
/// Getting the current core id adds cycles, so I created a version where the core id can be passed down.
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
/// and sample_count specifies how many samples should be recorded. Clears previous values in IP_LIST and TASK_ID_LIST so values must be retrieved before 
/// starting a new sampling process. 
/// # Note
/// Currently can only sample "this" core - core that the function was called on. 
pub fn start_samples(event_type: EventType, event_per_sample: u32, task_id: Option<usize>, sample_count: u32) -> Result<(),&'static str> {        
    check_pmu_availability()?;

    // perform checks to ensure that counter is ready to use and that previous results are not being unintentionally discarded
    if let Some(my_core_id) = apic::get_my_apic_id() {
        let mut cores_sampling_locked = CORES_SAMPLING.lock();
        if cores_sampling_locked.contains(&my_core_id) {
            return Err("Sampling is already being performed on this CPU.");
        }
        else {
            if RESULTS_READY.lock().contains(&my_core_id) {
                return Err("Sample results from previous test have not yet been retrieved. Please use retrieve_samples() function or 
                set RESULTS_READY AtomicBool for this cpu as false to indicate intent to discard sample results.");
            }
            cores_sampling_locked.insert(my_core_id);
        }
    }
    
    IP_LIST.lock().clear();
    TASK_ID_LIST.lock().clear();
    // if a task_id was requested, sets the value for the interrupt handler to later read
    if let Some(task_id_num) = task_id {
        SAMPLE_TASK_ID.store(task_id_num, Ordering::SeqCst);
    }
    SAMPLE_COUNT.store(sample_count, Ordering::SeqCst);
    if event_per_sample > core::u32::MAX || event_per_sample <= core::u32::MIN {
        return Err("Number of events per sample invalid: must be within unsigned 32 bit");
    }
    let start_value = core::u32::MAX - event_per_sample;
    SAMPLE_START_VALUE.store(start_value as usize, Ordering::SeqCst);

    // selects the appropriate mask for the event type and starts the counter
    let event_mask = match event_type {
        EventType::InstructionsRetired => UNHALTED_CYCLE_MASK,
        EventType::UnhaltedThreadCycles => INST_RETIRED_MASK,
        EventType::UnhaltedReferenceCycles => UNHALTED_REF_CYCLE_MASK,
        EventType::LongestLatCacheRef => LLC_REF_MASK,
        EventType::LongestLatCacheMiss => LLC_MISS_MASK,
        EventType::BranchInstructionsRetired => BR_INST_RETIRED_MASK,
        EventType::BranchMispredictRetired => BR_MISS_RETIRED_MASK,	
    } | PMC_ENABLE | INTERRUPT_ENABLE;
    unsafe{
        wrmsr(IA32_PMC0, start_value as u64);
        wrmsr(IA32_PERFEVTSEL0, event_mask | PMC_ENABLE | INTERRUPT_ENABLE);
    }

    return Ok(());

}

pub struct SampleResults {
    pub instruction_pointers: Vec<VirtualAddress>,
    pub task_ids:  Vec<usize>,
}

/// Function to manually stop the sampling interrupts. Marks the stored instruction pointers and task IDs as ready to retrieve. 
pub fn stop_samples() -> Result<(), &'static str> {
    // immediately stops counting and clears the counter
    unsafe{
        wrmsr(IA32_PERFEVTSEL0, 0);
        wrmsr(IA32_PMC0, 0);
        wrmsr(IA32_PERF_GLOBAL_OVF_CTRL, 0);

    }
    // clears values in atomics so that even if exception is somehow triggered, it stops at the next iteration
    SAMPLE_START_VALUE.store(0, Ordering::SeqCst);
    SAMPLE_TASK_ID.store(0, Ordering::SeqCst);
    // marks core as no longer sampling and results as ready
    if let Some(my_core_id) = apic::get_my_apic_id() {
        let mut locked_cores_sampling = CORES_SAMPLING.lock();
        RESULTS_READY.lock().insert(my_core_id);
        locked_cores_sampling.remove(&my_core_id);
        return Ok(());
        
    } else {
        return Err("Task structure not yet enabled.");
    }
}

/// Returns the samples that were stored during sampling in the form of a SampleResults object. 
/// If samples are not yet finished, forces them to stop.  
pub fn retrieve_samples() -> Result<SampleResults, &'static str> {
    if let Some(my_core_id) = apic::get_my_apic_id() {
        stop_samples()?;
        
        RESULTS_READY.lock().remove(&my_core_id);
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

pub fn handle_sample(stack_frame: &mut ExceptionStackFrame) {
    
    debug!("handle_sample(): [0] on core {:?}!", apic::get_my_apic_id());

    let event_mask = rdmsr(IA32_PERFEVTSEL0);
    let current_count = SAMPLE_COUNT.load(Ordering::SeqCst);
    // if all samples have already been taken, calls the function to turn off the counter

    debug!("handle_sample(): [1] on core {:?}!", apic::get_my_apic_id());
    if current_count == 0 {
        if stop_samples().is_err() {
            debug!("Error stopping samples. Counter not marked as free.");
        } 
        return;
    }
    SAMPLE_COUNT.store(current_count - 1, Ordering::SeqCst);

    debug!("handle_sample(): [2] on core {:?}!", apic::get_my_apic_id());

    // if the running task is the requested one or if one isn't requested, records the IP
    if let Some(taskref) = task::get_my_current_task() {

        debug!("handle_sample(): [3] on core {:?}!", apic::get_my_apic_id());

        let requested_task_id = SAMPLE_TASK_ID.load(Ordering::SeqCst);

        debug!("handle_sample(): [4] on core {:?}!", apic::get_my_apic_id());
        
        let task_id = taskref.lock().id;
        if (requested_task_id == 0) | (requested_task_id == task_id) {
            IP_LIST.lock().push(stack_frame.instruction_pointer);
            TASK_ID_LIST.lock().push(task_id);
        }
    } else {

        debug!("handle_sample(): [5] on core {:?}!", apic::get_my_apic_id());

        IP_LIST.lock().push(stack_frame.instruction_pointer);

        debug!("handle_sample(): [6] on core {:?}!", apic::get_my_apic_id());

        TASK_ID_LIST.lock().push(0);
    }
    // stops the counter, resets it, and restarts it
    unsafe {
        wrmsr(IA32_PERFEVTSEL0, 0);
        wrmsr(IA32_PERF_GLOBAL_STAUS, 0);
        wrmsr(IA32_PMC0, SAMPLE_START_VALUE.load(Ordering::SeqCst) as u64);
        wrmsr(IA32_PERFEVTSEL0, event_mask);
    }

    debug!("handle_sample(): [end] on core {:?}!", apic::get_my_apic_id());

    return;
}

/// Read 64 bit PMC (performance monitor counter).
pub fn rdpmc(msr: u32) -> u64 {
    let (high, low): (u32, u32);
    unsafe {
        asm!("rdpmc": "={eax}" (low), "={edx}" (high): "{ecx}" (msr) : "memory" : "volatile");
    }
    ((high as u64) << 32) | (low as u64)
}
