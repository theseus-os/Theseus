//! Support for the Performance Monitoring Unit 
//! 
//! We have support for PMU version 2. Each succesive PMU version includes the features provided by the previous versions.
//! 
//! Version 1 Support:
//! To configure an architectural performance monitoring event, we program the performance event select registers (IA32_PERFEVTSELx MSRs).
//! The result of the performance monitoring event is reported in a general purpose Performance Monitoring Counter (PMC) (IA32_PMCx MSR).
//! There is one PMC for each performance event select register, and one PMU per logical core. 
//! 
//! Version 2 Support:
//! Three of the architectural events are counted using fixed function MSRs (IA32_FIXED_CTR0 through IA32_FIXED_CTR2), 
//! each with an associated control register.
//! Three more MSRS are provided to simplify event programming. They are: 
//! * IA32_PERF_GLOBAL_CTRL: 
//!     allows software to enable/disable event counting of any combination of fixed-function PMCs or any general-purpose PMCs via a single WRMSR.
//! * IA32_PERF_GLOBAL_STATUS: 
//!     allows software to query counter overflow conditions on any combination of fixed-function PMCs or general-purpose PMCs via a single RDMSR.
//! * IA32_PERF_GLOBAL_OVF_CTRL: 
//!     allows software to clear counter overflow conditions on any combination of fixed-function PMCs or general-purpose PMCs via a single WRMSR.
//! 
//! We support 2 ways to use the PMU. One is to measure the number of events that take place over a length of code.
//! The second is Event Based Sampling, where after a specified number of events occur, an interrupt is called and we store the instruction pointer 
//! and task id running at that point.
//! 
//! Currently we support a maximum core ID of 255, and up to 8 general purpose counters per core. 
//! A core ID greater than 255 is not supported in Theseus in general since the ID has to fit within a u8.
//! 
//! If the core ID limit is changed and we need to update the PMU data structures to support more cores then: 
//! - Increase WORDS_IN_BITMAP and CORES_SUPPORTED_BY_PMU as required. For example, the cores supported is 256 so there are 4 64-bit words in the bitmap, one bit per core. 
//! - Add additional AtomicU64 variables to the initialization of the CORES_SAMPLING and RESULTS_READY bitmaps. 
//! 
//! If the general purpose PMC limit is reached then: 
//! - Update PMCS_SUPPORTED_BY_PMU to the new PMC limit.
//! - Change the element type in the PMCS_AVAILABLE vector to be larger than AtomicU8 so that there is one bit per counter.
//! - Update INIT_PMCS_AVAILABLE to the new maximum value for the per core bitmap.
//! 
//! Monitoring without interrupts is almost free (around 0.3% performance penalty) - source: "These are Not Your Grand Daddy's CPU Performance Counters" Blackhat USA, 2015
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

#![no_std]
#![feature(const_btree_new)]

extern crate spin;
#[macro_use] extern crate lazy_static;
extern crate x86_64;
extern crate msr;
extern crate raw_cpuid;
extern crate task;
extern crate memory;
extern crate irq_safety;
extern crate alloc;
extern crate apic;
#[macro_use] extern crate log;
extern crate mod_mgmt;
extern crate bit_field;

use msr::*;
use x86_64::{VirtAddr, registers::model_specific::Msr, structures::idt::InterruptStackFrame};
use raw_cpuid::*;
use spin::Once;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::{String, ToString};
use bit_field::BitField;
use core::sync::atomic::{Ordering, AtomicU64, AtomicU8};

pub mod stat;

/// The minimum version ID a PMU can have, as retrieved by cpuid. Anything lower than this means a PMU is not supported.
const MIN_PMU_VERSION: u8 = 1;
/// Set bits in the global_ctrl MSR to enable the fixed counters.
const ENABLE_FIXED_PERFORMANCE_COUNTERS: u64 = 0x7 << 32;
/// Set bits in the global_ctrl MSR to enable the general purpose PMCs.
const ENABLE_GENERAL_PERFORMANCE_COUNTERS: u64 = 0xF;
/// Set bits in fixed_ctr_ctrl MSR to enable fixed counters for all privilege levels. 
const ENABLE_FIXED_COUNTERS_FOR_ALL_PRIVILEGE_LEVELS: u64 = 0x333;

/// read from the fixed counter 0 to retrieve the instructions retired
const FIXED_FUNC_0_RDPMC: u32 = 1 << 30;
/// read from the fixed counter 1 to retrieve the clock cycles
const FIXED_FUNC_1_RDPMC: u32 = (1 << 30) + 1;
/// read from the fixed counter 2 to retrieve reference cycles
const FIXED_FUNC_2_RDPMC: u32 = (1 << 30) + 2;

/// Set bit 22 to enable the general PMC to start counting
const PMC_ENABLE: u64 = 0x01 << 22;
/// Set bit 20 in the event select register to enable interrupts on a counter overflow
const INTERRUPT_ENABLE: u64 = 0x01 << 20;
/// Value to write to the overflow control MSR to clear it
const CLEAR_PERF_STATUS_MSR: u64 = 0x0000_0000_0000_000F;
/// The number of words in the CORES_SAMPLING and RESULTS_READY bitmaps, so that information for 256 cores can be recorded
const WORDS_IN_BITMAP: usize = 4;
/// The number of cores the current PMU implementation supports
const CORES_SUPPORTED_BY_PMU: usize = 256;
/// The number of general purpose PMCs the current PMU implementation supports
const PMCS_SUPPORTED_BY_PMU: u8 = 8;
/// The initial value for the bitmaps in PMCS_AVAILABLE 
const INIT_VAL_PMCS_AVAILABLE: u8 = core::u8::MAX;

/// Stores an 8-bit bitmap for each core to show whether the PMCs for that core are available (1) or in use (0).
/// This restricts us to 8 general purpose PMCs per core.
static PMCS_AVAILABLE: Once<Vec<AtomicU8>> = Once::new();
/// Bitmap to store the cores where the PMU is currently being used for sampling. It records information for 256 cores.
static CORES_SAMPLING: [AtomicU64; WORDS_IN_BITMAP] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
/// Bitmap to store the cores which have sampling results ready to be retrieved. It records information for 256 cores.
static RESULTS_READY: [AtomicU64; WORDS_IN_BITMAP] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

lazy_static! {
    /// PMU version supported by the current hardware. The default is zero since the performance monitoring information would not be retrieved only if there was no PMU available.
    static ref PMU_VERSION: u8 = CpuId::new().get_performance_monitoring_info().map(|pmi| pmi.version_id()).unwrap_or(0);
    /// The number of general purpose PMCs that can be used. The default is zero since the performance monitoring information would not be retrieved only if there was no PMU available.
    static ref NUM_PMC: u8 = CpuId::new().get_performance_monitoring_info().map(|pmi| pmi.number_of_counters()).unwrap_or(0);
    /// The number of fixed function counters. The default is zero since the performance monitoring information would not be retrieved only if there was no PMU available.
    static ref NUM_FIXED_FUNC_COUNTERS: u8 = CpuId::new().get_performance_monitoring_info().map(|pmi| pmi.fixed_function_counters()).unwrap_or(0);
}

/// Set to store the cores that the PMU has already been initialized on
static CORES_INITIALIZED: MutexIrqSafe<BTreeSet<u8>> = MutexIrqSafe::new(BTreeSet::new());
/// The sampling information for each core
static SAMPLING_INFO: MutexIrqSafe<BTreeMap<u8, SampledEvents>> =  MutexIrqSafe::new(BTreeMap::new());

/// Used to select the event type to count. Event types are described in the Intel SDM 18.2.1 for PMU Version 1.
/// The discriminant value for each event type is the value written to the event select register for a general purpose PMC.
pub enum EventType{
    /// This event counts the number of instructions at retirement. For instructions that consist of multiple micro-ops,
    /// this event counts the retirement of the last micro-op of the instruction.
    /// This is counted by IA32_FIXED_CTR0.
    InstructionsRetired = (0x03 << 16) | (0x00 << 8) | 0xC0,
    /// This event counts core clock cycles when the clock signal on a specific core is running (not halted).
    /// This is counted by IA32_FIXED_CTR1. 
    UnhaltedCoreCycles = (0x03 << 16) | (0x00 << 8) | 0x3C,
    /// This event counts reference clock cycles at a fixed frequency while the clock signal on the core is running. 
    /// The event counts at a fixed frequency, irrespective of core frequency changes due to performance state transitions. 
    /// Current implementations use the TSC clock.
    /// This is counted by IA32_FIXED_CTR2. 
    UnhaltedReferenceCycles = (0x03 << 16) | (0x01 << 8) | 0x3C,
    /// This event counts requests originating from the core that reference a cache line in the last level on-die cache.
    /// The event count includes speculation and cache line fills due to the first-level cache hardware prefetcher, but
    /// may exclude cache line fills due to other hardware-prefetchers.
    LastLevelCacheReferences = (0x03 << 16) | (0x4F << 8) | 0x2E,
    /// This event counts each cache miss condition for references to the last level on-die cache. The event count may
    /// include speculation and cache line fills due to the first-level cache hardware prefetcher, but may exclude cache
    /// line fills due to other hardware-prefetchers.
    LastLevelCacheMisses = (0x03 << 16) | (0x41 << 8) | 0x2E,
    /// This event counts branch instructions at retirement. It counts the retirement of the last micro-op of a branch instruction.
    BranchInstructionsRetired = (0x03 << 16) | (0x00 << 8) | 0xC4,
    /// This event counts mispredicted branch instructions at retirement. It counts the retirement of the last micro-op
    /// of a branch instruction in the architectural path of execution and experienced misprediction in the branch
    /// prediction hardware.
    BranchMissesRetired = (0x03 << 16) | (0x00 << 8) | 0xC5,
}

fn num_general_purpose_counters() -> u8 {
    *NUM_PMC
}

fn get_pmcs_available() -> Result<&'static Vec<AtomicU8>, &'static str>{
    Ok(PMCS_AVAILABLE.get().ok_or("pmu_x86: The variable storing the available counters for each core hasn't been initialized")?)
}

fn get_pmcs_available_for_core(core_id: u8) -> Result<&'static AtomicU8, &'static str>{
    let pmc = PMCS_AVAILABLE.get().ok_or("pmu_x86: The variable storing the available counters for each core hasn't been initialized")?;
    Ok(&pmc[core_id as usize])
}

/// Returns the maximum core id for this machine
fn max_core_id() -> Result<u8, &'static str> {
    let lapics = apic::get_lapics();
    let core = lapics.iter().max_by_key(|core| core.0).ok_or("pmu_x86: Could not find a maximum core id")?;
    Ok(*core.0)
}

/// Returns true if there are core ids that are larger than can be handled by the current PMU data structures
fn greater_core_id_than_expected() -> Result<bool, &'static str> {
    let max_core = max_core_id()? as usize;
    Ok(max_core >= CORES_SUPPORTED_BY_PMU)
}

/// Returns true if there are more general purpose PMCs than can be handled by the current PMU data structures
fn more_pmcs_than_expected(num_pmc: u8) -> Result<bool, &'static str> {
    Ok(num_pmc > PMCS_SUPPORTED_BY_PMU)
}


/// Initialization function that enables the PMU if one is available.
/// We initialize the 3 fixed PMCs and general purpose PMCs. Calling this initialization function again
/// on a core that has already been initialized will do nothing.
/// 
/// Currently we support a maximum core ID of 255, and up to 8 general purpose counters per core. 
/// A core ID greater than 255 is not supported in Theseus in general since the ID has to fit within a u8.
/// 
/// If the core ID limit is changed and we need to update the PMU data structures to support more cores then: 
/// - Increase WORDS_IN_BITMAP and CORES_SUPPORTED_BY_PMU as required. For example, the cores supported is 256 so there are 4 64-bit words in the bitmap, one bit per core. 
/// - Add additional AtomicU64 variables to the initialization of the CORES_SAMPLING and RESULTS_READY bitmaps. 
/// 
/// If the general purpose PMC limit is reached then: 
/// - Update PMCS_SUPPORTED_BY_PMU to the new PMC limit.
/// - Change the element type in the PMCS_AVAILABLE vector to be larger than AtomicU8 so that there is one bit per counter.
/// - Update INIT_PMCS_AVAILABLE to the new maximum value for the per core bitmap.
/// 
/// # Warning
/// This function should only be called after all the cores have been booted up.
pub fn init() -> Result<(), &'static str> {
    let mut cores_initialized = CORES_INITIALIZED.lock();
    let core_id = apic::get_my_apic_id();

    if cores_initialized.contains(&core_id) {
        warn!("PMU has already been intitialized on core {}", core_id);
        return Ok(());
    } 
    else {

        if *PMU_VERSION >= MIN_PMU_VERSION {
                
            if greater_core_id_than_expected()? {
                return Err("pmu_x86: There are larger core ids in this machine than can be handled by the existing structures which store information about the PMU");
            }

            if more_pmcs_than_expected(*NUM_PMC)? {
                return Err("pmu_x86: There are more general purpose PMCs in this machine than can be handled by the existing structures which store information about the PMU");
            }

            PMCS_AVAILABLE.call_once(|| {
                // initialize the PMCS_AVAILABLE bitmap 
                let core_capacity = max_core_id().expect("cores have not been booted up and so cannot retrieve a max core id") as usize + 1;
                let mut pmcs_available = Vec::with_capacity(core_capacity);
                for _ in 0..core_capacity {
                    pmcs_available.push(AtomicU8::new(INIT_VAL_PMCS_AVAILABLE));
                } 
                trace!("PMU initialized for the first time: version: {} with fixed counters: {} and general counters: {}", *PMU_VERSION, *NUM_FIXED_FUNC_COUNTERS, *NUM_PMC);
                pmcs_available
            });
        }
        else {
            error!("This machine does not support a PMU");
            return Err("This machine does not support a PMU");
        }
        
        init_registers();
        cores_initialized.insert(core_id);
        trace!("PMU initialized on core {}", core_id);
    }
    
    Ok(())
}

/// Part of the initialization routine which actually does the work of setting up the registers.
/// This must be called for every core that wants to use the PMU.
fn init_registers() {
    unsafe {
        // disables all the performance counters
        Msr::new(IA32_PERF_GLOBAL_CTRL).write(0);
        // clear the general purpose PMCs
        Msr::new(IA32_PMC0).write(0);
        Msr::new(IA32_PMC1).write(0);
        Msr::new(IA32_PMC2).write(0);
        Msr::new(IA32_PMC3).write(0);
        // clear the fixed event counters
        Msr::new(IA32_FIXED_CTR0).write(0);
        Msr::new(IA32_FIXED_CTR1).write(0);
        Msr::new(IA32_FIXED_CTR2).write(0);
        // sets fixed function counters to count events at all privilege levels
        Msr::new(IA32_FIXED_CTR_CTRL).write(ENABLE_FIXED_COUNTERS_FOR_ALL_PRIVILEGE_LEVELS);
        // enables all counters: each counter has another enable bit in other MSRs so these should likely never be cleared once first set
        Msr::new(IA32_PERF_GLOBAL_CTRL).write(ENABLE_FIXED_PERFORMANCE_COUNTERS | ENABLE_GENERAL_PERFORMANCE_COUNTERS);
    }
}

/// A logical counter object to correspond to a physical PMC
pub struct Counter {
    /// Initial value stored in the counter before counting starts
    start_count: u64,
    /// value passed to rdpmc instruction to read counter value
    msr_mask: u32,
    /// General PMC register number. It is -1 for a fixed counter.
    pmc: i32,
    /// Core this PMC is a part of.
    core: u8,
}

impl Counter {
    /// Creates a Counter object and assigns a physical counter for it. 
    /// If it's a general PMC, writes the UMASK and Event Code to the relevant MSR, but leaves enable bit clear.  
    pub fn new(event: EventType) -> Result<Counter, &'static str> {
        // ensures PMU available and initialized
        check_pmu_availability()?;
        
        match event {
            EventType::InstructionsRetired => create_fixed_counter(FIXED_FUNC_0_RDPMC),
            EventType::UnhaltedCoreCycles => create_fixed_counter(FIXED_FUNC_1_RDPMC),
            EventType::UnhaltedReferenceCycles => create_fixed_counter(FIXED_FUNC_2_RDPMC),
            _ => create_general_counter(event as u64)	
        }
    }
    
    /// Starts the count.
    pub fn start(&mut self) -> Result<(), &'static str> {
        let num_pmc = num_general_purpose_counters();

        // for a fixed value counter (that's already enabled), simply saves the current value as the starting count
        if self.msr_mask > num_pmc as u32 {
            self.start_count = rdpmc(self.msr_mask);
        } 
        // for a general PMC, it enables the counter to start counting from 0
        else {
            self.start_count = 0;
            unsafe { 
                let umask = Msr::new(IA32_PERFEVTSEL0 + self.pmc as u32).read();
                Msr::new(IA32_PERFEVTSEL0 + self.pmc as u32).write(umask | PMC_ENABLE);
            }
        }
        Ok(())
    }
    
    /// Allows user to get count since start without stopping/releasing the counter.
    pub fn get_count_since_start(&self) -> Result<u64, &'static str> {
        let num_pmc = num_general_purpose_counters();

        //checks to make sure the counter hasn't already been released
        if self.msr_mask < num_pmc as u32 {
            if counter_is_available(self.core, self.msr_mask as u8)? {
                return Err("Counter used for this event was marked as free, value stored is likely inaccurate.");
            } 
        }
        
        Ok(rdpmc(self.msr_mask) - self.start_count)
    }
    
    /// Stops counting, releases the counter, and returns the count of events since the counter was initialized.
    /// This will consume the counter object since after freeing the counter, the counter should not be accessed.
    pub fn end(self) -> Result<u64, &'static str> {
        let end_val = rdpmc(self.msr_mask);
        let start_count = self.start_count;
        drop(self);
        Ok(end_val - start_count)
    }

    /// lightweight function with no checks to get the counter value from when it was started.
    pub fn diff(&self) -> u64 {
        rdpmc(self.msr_mask) - self.start_count
    }
}

impl Drop for Counter {
    fn drop(&mut self) {
        let num_pmc = num_general_purpose_counters(); 

        // A programmable counter would be claimed at this point, so free it now so it can be used again.
        // Otherwise the counter is a fixed function counter and nothing needs to be done.
        if self.msr_mask < num_pmc as u32 {
            // clears event counting settings and counter 
            unsafe{
                Msr::new(IA32_PERFEVTSEL0 + self.msr_mask as u32).write(0);
                Msr::new(IA32_PMC0 + self.msr_mask as u32).write(0);
            }
            free_counter(self.core, self.msr_mask as u8); 
        }
    }

}

/// Returns true if the counter is not in use
fn counter_is_available(core_id: u8, counter: u8) -> Result<bool, &'static str> {
    let pmcs = get_pmcs_available_for_core(core_id)?;
    Ok(pmcs.load(Ordering::SeqCst).get_bit(counter as usize))
}

/// Sets the counter bit to indicate it is available
fn free_counter(core_id: u8, counter: u8) {
    let pmcs = get_pmcs_available_for_core(core_id).expect("Trying to free a PMU counter when the PMU is not initialized");

    pmcs.fetch_update(
        Ordering::SeqCst,
        Ordering::SeqCst,
        |mut x| {
            if !x.get_bit(counter as usize) {
                x.set_bit(counter as usize, true);
                Some(x)
            }
            else {
                None
            }
        }
    ).unwrap_or_else(|x| { 
        warn!("The counter you are trying to free has been previously freed"); 
        x
    });
}

/// Clears the counter bit to show it's in use
fn claim_counter(core_id: u8, counter: u8) -> Result<(), &'static str> {
    let pmcs = get_pmcs_available_for_core(core_id)?;

    pmcs.fetch_update(
        Ordering::SeqCst,
        Ordering::SeqCst,
        |mut x| {
            if x.get_bit(counter as usize) {
                x.set_bit(counter as usize, false);
                Some(x)
            }
            else {
                None
            }
        }
    ).map_err(|_e| "pmu_x86: Could not claim counter because it is already in use")?;

    Ok(())
}

/// Frees all counters and make them available to be used.
/// Essentially sets the PMU to its initial state.
pub fn reset_pmu() -> Result<(), &'static str> {
    for pmc in get_pmcs_available()?.iter() {
        pmc.store(INIT_VAL_PMCS_AVAILABLE, Ordering::SeqCst);
    }
    for core in CORES_SAMPLING.iter() {
        core.store(0, Ordering::SeqCst);
    }
    for result in RESULTS_READY.iter() {
        result.store(0, Ordering::SeqCst);
    }
    SAMPLING_INFO.lock().clear();

    Ok(())
}

/// Returns the word position and the bit in the word for the given bit number
/// which will be used to access the bitmap. 
fn find_word_and_offset_from_bit(bit_num: u8) -> (usize, usize) {
    let total_bits_in_word = core::mem::size_of::<u64>() * 8;
    let word_num = bit_num as usize / total_bits_in_word;
    let bit_in_word = bit_num as usize % total_bits_in_word;
    (word_num, bit_in_word)
}

/// Adds the core to the list of those where PMU sampling is underway
/// by setting the core_id bit
fn add_core_to_sampling_list(core_id: u8) -> Result<(), &'static str> {
    let (word_num, bit_in_word) = find_word_and_offset_from_bit(core_id);

    CORES_SAMPLING[word_num].fetch_update(
        Ordering::SeqCst,
        Ordering::SeqCst,
        |mut x| {
            if !x.get_bit(bit_in_word) {
                x.set_bit(bit_in_word, true);
                Some(x)
            }
            else{
                None
            }
        }
    ).map_err(|_e| "pmu_x86: could not add core to sampling list since sampling is already started on this core")?;

    Ok(())
}

/// Removes the core from the list of those where PMU sampling is underway
/// by clearing the core_id bit
fn remove_core_from_sampling_list(core_id: u8) -> Result<(), &'static str> {
    let (word_num, bit_in_word) = find_word_and_offset_from_bit(core_id);

    CORES_SAMPLING[word_num].fetch_update(
        Ordering::SeqCst,
        Ordering::SeqCst,
        |mut x| {
            if x.get_bit(bit_in_word) {
                x.set_bit(bit_in_word, false);
                Some(x)
            }
            else {
                None
            }
        }
    ).map_err(|_e| "pmu_x86: could not remove core from sampling list since sampling has already finished on this core")?;

    Ok(())
}

fn core_is_currently_sampling(core_id: u8) -> bool {
    let (word_num, bit_in_word) = find_word_and_offset_from_bit(core_id);
    CORES_SAMPLING[word_num].load(Ordering::SeqCst).get_bit(bit_in_word)
}

/// Notify that sampling results are ready to be retrieved by setting the core_id bit
fn notify_sampling_results_are_ready(core_id: u8) -> Result<(), &'static str> {
    let (word_num, bit_in_word) = find_word_and_offset_from_bit(core_id);

    RESULTS_READY[word_num].fetch_update(
        Ordering::SeqCst,
        Ordering::SeqCst,
        |mut x| {
            if !x.get_bit(bit_in_word) {
                x.set_bit(bit_in_word, true);
                Some(x)
            }
            else {
                None
            }
        }
    ).map_err(|_e|"pmu_x86: could not add core to results ready list as sampling results have already been added")?;

    Ok(())
}

/// Notify that sampling results have been retrieved by clearing the core_id bit
fn sampling_results_have_been_retrieved(core_id: u8) -> Result<(), &'static str> {
    let (word_num, bit_in_word) = find_word_and_offset_from_bit(core_id);

    RESULTS_READY[word_num].fetch_update(
        Ordering::SeqCst,
        Ordering::SeqCst,
        |mut x| {
            if x.get_bit(bit_in_word) {
                x.set_bit(bit_in_word, false);
                Some(x)
            }
            else {
                None
            }
        }
    ).map_err(|_e| "pmu_x86: could not remove core from results ready list as sampling results have already been retrieved")?;

    Ok(())
}

fn sampling_results_are_ready(core_id: u8) -> bool {
    let (word_num, bit_in_word) = find_word_and_offset_from_bit(core_id);
    RESULTS_READY[word_num].load(Ordering::SeqCst).get_bit(bit_in_word)
}

/// Creates a counter object for a general purpose PMC given the event type.
fn create_general_counter(event_mask: u64) -> Result<Counter, &'static str> {
    programmable_start(event_mask)
}

/// Does the work of iterating through programmable counters and using whichever one is free. Returns Err if none free
fn programmable_start(event_mask: u64) -> Result<Counter, &'static str> {
    let my_core = apic::get_my_apic_id();
    let num_pmc = num_general_purpose_counters();

    for pmc in 0..num_pmc {
        //If counter i on this core is currently in use, checks the next counter
        if !counter_is_available(my_core, pmc)? {
            continue;
        }
        //Claims the counter using the AtomicMap and writes the values to initialize counter (except for enable bit)
        claim_counter(my_core, pmc)?;

        unsafe{
            Msr::new(IA32_PMC0 + (pmc as u32)).write(0);
            Msr::new(IA32_PERFEVTSEL0 + (pmc as u32)).write(event_mask);
        }
        return Ok(Counter {
            start_count: 0, 
            msr_mask: pmc as u32, 
            pmc: pmc as i32, 
            core: my_core
        });
    }
    return Err("All programmable counters currently in use.");
}

/// Creates a counter object for a fixed hardware counter
fn create_fixed_counter(msr_mask: u32) -> Result<Counter, &'static str> {
    let my_core = apic::get_my_apic_id();
    let count = rdpmc(msr_mask);
    
    return Ok(Counter {
        start_count: count, 
        msr_mask: msr_mask, 
        pmc: -1, 
        core: my_core
    });
}

/// Checks that the PMU has been initialized. If it has been,
/// the version ID tells whether the system has performance monitoring capabilities. 
fn check_pmu_availability() -> Result<(), &'static str>  {
    let core_id = apic::get_my_apic_id();
    if !CORES_INITIALIZED.lock().contains(&core_id) {
        if *PMU_VERSION >= MIN_PMU_VERSION {
            error!("PMU version {} is available. It still needs to be initialized on this core", *PMU_VERSION);
            return Err("PMU is available. It still needs to be initialized on this core");
        }
        else {
            error!("This machine does not support a PMU");
            return Err("This machine does not support a PMU");
        }
    }

    Ok(())
}



/// The information stored for each core when event based sampling is in progress
struct SampledEvents{
    start_value: usize,
    task_id: usize,
    sample_count: u32,
    ip_list: Vec<VirtAddr>,
    task_id_list: Vec<usize>,
}

impl SampledEvents {
    pub fn with_capacity(capacity: usize) -> SampledEvents {
        SampledEvents {
            start_value: 0,
            task_id: 0,
            sample_count: 0,
            ip_list: Vec::with_capacity(capacity),
            task_id_list: Vec::with_capacity(capacity),
        }
    }
}

/// Start interrupt process in order to take samples using the PMU. 
/// It loads the starting value as such that an overflow will occur at "event_per_sample" events. 
/// That overflow triggers an interrupt where information about the current running task is sampled.
/// 
/// # Arguments
/// * `event_type`: determines type of event that sampling interval is based on
/// * `event_per_sample`: how many of those events should occur between each sample 
/// * `task_id`: allows the user to choose to only sample from a specific task by inputting a number or sample from all tasks by inputting None 
/// * `sample_count`: specifies how many samples should be recorded. 
/// 
/// # Note
/// The function clears any previous values from sampling stored for this core, so values must be retrieved before starting a new sampling process.
/// Currently can only sample "this" core - core that the function was called on. 
/// 
pub fn start_samples(event_type: EventType, event_per_sample: u32, task_id: Option<usize>, sample_count: u32) -> Result<(),&'static str> {        
    check_pmu_availability()?;

    // perform checks to ensure that counter is ready to use and that previous results are not being unintentionally discarded
    let my_core_id = apic::get_my_apic_id();

    trace!("start_samples: the core id is : {}", my_core_id);

    // If counter 0 is currently in use (false), then return 
    // PMC0 is always used for sampling, but is not reserved for it.
    if !counter_is_available(my_core_id, 0)? {
        return Err("PMU counter 0 is currently in use and can't be used for sampling. End all other PMU tasks and try again");
    }

    // set the counter as in use
    claim_counter(my_core_id, 0)?;

    // check to make sure that sampling is not already in progress on this core
    // this should never happen since the counter would not be available for use
    if core_is_currently_sampling(my_core_id) {
        return Err("Sampling is already being performed on this CPU.");
    }
    else {
        // check if there are previous sampling results that haven't been retrieved
        if sampling_results_are_ready(my_core_id) {
            return Err("Sample results from previous test have not yet been retrieved. Please use retrieve_samples() function or 
            set RESULTS_READY AtomicBool for this cpu as false to indicate intent to discard sample results.");
        }
        add_core_to_sampling_list(my_core_id)?;
    }
    
    let mut sampled_events = SampledEvents::with_capacity(sample_count as usize);

    // if a task_id was requested, sets the value for the interrupt handler to later read
    if let Some(task_id_num) = task_id {
        sampled_events.task_id = task_id_num;
    }

    sampled_events.sample_count = sample_count;

    if event_per_sample == 0 {
        return Err("Number of events per sample invalid: must be nonzero");
    }
    // This check can never trigger since `event_per_sample` is a `u32`
    // and is therefore by definition in the range `u32::MIN..=u32::MAX`.
    // We'll check anyways, just in case `event_per_sample`'s type is changed.
    #[allow(clippy::absurd_extreme_comparisons)]
    if event_per_sample > core::u32::MAX || event_per_sample < core::u32::MIN {
        return Err("Number of events per sample invalid: must be within unsigned 32 bit");
    }

    let start_value = core::u32::MAX - event_per_sample;
    // SAMPLE_START_VALUE.store(start_value as usize, Ordering::SeqCst);
    sampled_events.start_value = start_value as usize;

    // add this sampled events to the atomic map
    SAMPLING_INFO.lock().insert(my_core_id, sampled_events);

    // selects the appropriate mask for the event type and starts the counter
    let event_mask = event_type as u64;

    unsafe{
        Msr::new(IA32_PMC0).write(start_value as u64);
        Msr::new(IA32_PERFEVTSEL0).write(event_mask | PMC_ENABLE | INTERRUPT_ENABLE);
    }

    return Ok(());

}

/// Function to manually stop the sampling interrupts. Marks the stored instruction pointers and task IDs as ready to retrieve. 
fn stop_samples(core_id: u8, samples: &mut SampledEvents) -> Result<(), &'static str> {
    // immediately stops counting and clears the counter
    unsafe{
        Msr::new(IA32_PERFEVTSEL0).write(0);
        Msr::new(IA32_PMC0).write(0);
        Msr::new(IA32_PERF_GLOBAL_OVF_CTRL).write(CLEAR_PERF_STATUS_MSR);
    }

    // clears values so that even if exception is somehow triggered, it stops at the next iteration
    samples.start_value = 0;
    samples.task_id = 0; 
    samples.sample_count = 0;

    // marks core as no longer sampling and results as ready
    remove_core_from_sampling_list(core_id)?;
    notify_sampling_results_are_ready(core_id)?;
    free_counter(core_id, 0);

    trace!("Stopped taking samples with the PMU");
    
    return Ok(());
}

/// Stores the instruction pointers and corresponding task IDs from the samples
pub struct SampleResults {
    pub instruction_pointers: Vec<memory::VirtualAddress>,
    pub task_ids:  Vec<usize>,
}

/// Returns the samples that were stored during sampling in the form of a SampleResults object. 
/// If samples are not yet finished, forces them to stop.  
pub fn retrieve_samples() -> Result<SampleResults, &'static str> {
    let my_core_id = apic::get_my_apic_id();

    let mut sampling_info = SAMPLING_INFO.lock();
    let mut samples = sampling_info.get_mut(&my_core_id).ok_or("pmu_x86::retrieve_samples: could not retrieve sampling information for this core")?;

    // the interrupt handler might have stopped samples already so thsi check is required
    if core_is_currently_sampling(my_core_id) {
        stop_samples(my_core_id, &mut samples)?;
    }
    
    sampling_results_have_been_retrieved(my_core_id)?;

    let mut instruction_pointers = Vec::with_capacity(samples.ip_list.len());
    instruction_pointers.extend(samples.ip_list.iter().map(|va| memory::VirtualAddress::new_canonical(va.as_u64() as usize)));

    Ok(SampleResults { instruction_pointers, task_ids: samples.task_id_list.clone() })   
}

/// Simple function to print values from SampleResults in a form that the script "post-mortem pmu analysis.py" can parse. 
pub fn print_samples(sample_results: &SampleResults) {
    debug!("Printing Samples:");
    for results in sample_results.instruction_pointers.iter().zip(sample_results.task_ids.iter()) {
        debug!("{:x} {}", results.0, results.1);
    }
}

/// Finds the corresponding function for each instruction pointer and calculates the percentage amount each function occured in the samples
pub fn find_function_names_from_samples(sample_results: &SampleResults) -> Result<(), &'static str> {
    let taskref = task::get_my_current_task().ok_or("pmu_x86::get_function_names_from_samples: Could not get reference to current task")?;
    let namespace = taskref.get_namespace();
    debug!("Analyze Samples:");

    let mut sections: BTreeMap<String, usize> = BTreeMap::new();
    let total_samples = sample_results.instruction_pointers.len();

    for ip in sample_results.instruction_pointers.iter() {
        let (section_ref, _offset) = namespace.get_section_containing_address(*ip, false)
            .ok_or("Can't find section containing sampled instruction pointer")?;
        let section_name = section_ref.name_without_hash().to_string();

        sections.entry(section_name).and_modify(|e| {*e += 1}).or_insert(1);
    }

    for (section, occurrences) in sections.iter() {
        let percentage = *occurrences as f32 / total_samples as f32 * 100.0;
        debug!("{:?}  {}% \n", section, percentage);
    }

    Ok(())
}

/// This function is designed to be invoked from an interrupt handler 
/// when a sampling interrupt has (or may have) occurred. 
///
/// It takes a sample by logging the the instruction pointer and task ID at the point
/// at which the sampling interrupt occurred. 
/// The counter is then either reset to its starting value 
/// (if there are more samples that need to be taken)
/// or disabled entirely if the final sample has been taken. 
///
/// # Argument
/// * `stack_frame`: the stack frame that was pushed onto the stack automatically 
///    by the CPU and passed into the interrupt/exception handler. 
///    This is used to determine during which instruction the sampling interrupt occurred.
///
/// # Return
/// * Returns `Ok(true)` if a PMU sample occurred and was handled. 
/// * Returns `Ok(false)` if PMU isn't supported, or if PMU wasn't yet initialized, 
///   or if there was not a pending sampling interrupt. 
/// * Returns an `Err` if PMU is supported and initialized and a sample was pending, 
///   but an error occurred while logging the sample.
///
pub fn handle_sample(stack_frame: &InterruptStackFrame) -> Result<bool, &'static str> {
    // Check that PMU hardware exists and is supported on this machine.
    if *PMU_VERSION < MIN_PMU_VERSION {
        return Ok(false);
    }
    // Check that a PMU sampling event is currently pending.
    if unsafe { Msr::new(IA32_PERF_GLOBAL_STAUS).read() } == 0 {
        return Ok(false);
    }

    unsafe { Msr::new(IA32_PERF_GLOBAL_OVF_CTRL).write(CLEAR_PERF_STATUS_MSR); }

    let my_core_id = apic::get_my_apic_id();

    let mut sampling_info = SAMPLING_INFO.lock();
    let mut samples = sampling_info.get_mut(&my_core_id)
        .ok_or("pmu_x86::handle_sample: Could not retrieve sampling information for this core")?;

    let current_count = samples.sample_count;
    // if all samples have already been taken, calls the function to turn off the counter
    if current_count == 0 {
        stop_samples(my_core_id, &mut samples)?; 
        return Ok(true);
    }

    samples.sample_count = current_count - 1;

    // if the running task is the requested one or if one isn't requested, records the IP
    if let Some(taskref) = task::get_my_current_task() {
        let requested_task_id = samples.task_id;
        
        let task_id = taskref.id;
        if (requested_task_id == 0) | (requested_task_id == task_id) {
            samples.ip_list.push(stack_frame.instruction_pointer);
            samples.task_id_list.push(task_id);
        }
    } else {
        samples.ip_list.push(stack_frame.instruction_pointer);
        samples.task_id_list.push(0);
    }

    // stops the counter, resets it, and restarts it
    unsafe {
        Msr::new(IA32_PERFEVTSEL0).write(0);
        Msr::new(IA32_PERF_GLOBAL_OVF_CTRL).write(CLEAR_PERF_STATUS_MSR);
        Msr::new(IA32_PMC0).write(samples.start_value as u64);
        Msr::new(IA32_PERFEVTSEL0).write(Msr::new(IA32_PERFEVTSEL0).read());
    }

    if let Some(my_apic) = apic::get_my_apic() {
        my_apic.write().clear_pmi_mask();
    }
    else {
        error!("Error in Performance Monitoring! Reference to the local APIC could not be retrieved.");
    }

    Ok(true)
}


/// Reads the given PMC (performance monitor counter) register.
fn rdpmc(msr: u32) -> u64 {
    let (high, low): (u32, u32);
    unsafe {
        core::arch::asm!(
            "rdpmc",
            in("ecx") msr,
            out("eax") low, out("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((high as u64) << 32) | (low as u64)
}
