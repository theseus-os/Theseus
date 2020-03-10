//! This module implements the equivalent of "perf stat".
//! Currently only 7 events are recorded.
//! 
//! 
//! # Example
//! ```
//! pmu_x86::init();
//! 
//! let counters = pmu_x86::stat::PerformanceCounters::new()?;
//! counters.start();
//! 
//! ...
//! // code to be measured
//! ...
//! 
//! let results = counters.end();
//! 
//! ```


use core::fmt;
use crate::*;

pub struct PerformanceCounters {
    inst_retired: Counter,
    core_cycles: Counter,
    ref_cycles: Counter,
    llc_ref: Counter,
    llc_miss: Counter,
    br_inst_ret: Counter,
    br_miss_ret: Counter,
}

pub struct PMUResults {
    inst_retired: u64,
    core_cycles: u64,
    ref_cycles: u64,
    llc_ref: u64,
    llc_miss: u64,
    br_inst_ret: u64,
    br_miss_ret: u64,
}

impl fmt::Debug for PMUResults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PMU stat \n 
        instructions retired:   {} \n 
        core cycles:    {} \n 
        reference cycles:   {} \n 
        llc references: {} \n 
        llc misses: {} \n 
        branch instructions retired:    {} \n 
        branch missed retired:  {} \n", 
        self.inst_retired, self.core_cycles, self.ref_cycles, self.llc_ref, self.llc_miss, self.br_inst_ret, self.br_miss_ret)
    }
}

impl PerformanceCounters {
    /// Initialize seven performance monitoring counters. They will measure:
    /// - Instructions retired
    /// - Core cycles
    /// - Reference cycles
    /// - LLC references
    /// - LLC misses 
    /// - Branch instructions retired
    /// - Branch misses retired
    pub fn new() -> Result<PerformanceCounters, &'static str> {
        // the first 3 fixed function counters do not need to rollback any changes
        let inst_retired = Counter::new(EventType::InstructionsRetired)?;
        let core_cycles = Counter::new(EventType::UnhaltedCoreCycles)?;    
        let ref_cycles = Counter::new(EventType::UnhaltedReferenceCycles)?;   

        // if any of the general purpose counters fail then we need to free the previously acquired counters
        let llc_ref = Counter::new(EventType::LastLevelCacheReferences);    
        if llc_ref.is_err() {
            return Err("pmu_x86::stat: Couldn't create llc ref counter, none available");            
        }   

        let llc_miss = Counter::new(EventType::LastLevelCacheMisses);
        if llc_miss.is_err() {
            llc_ref?.end()?;
            return Err("pmu_x86::stat: Couldn't create llc miss counter, none available");
        }

        let br_inst_ret = Counter::new(EventType::BranchInstructionsRetired);   
        if br_inst_ret.is_err() {
            llc_ref?.end()?;
            llc_miss?.end()?;
            return Err("pmu_x86::stat: Couldn't create branch inst retired counter, none available");
        }

        let br_miss_ret = Counter::new(EventType::BranchMissesRetired);
        if br_miss_ret.is_err() {
            llc_ref?.end()?;
            llc_miss?.end()?;
            br_inst_ret?.end()?;
            return Err("pmu_x86::stat: Couldn't create branch miss retired counter, none available");
        }
                
        Ok(PerformanceCounters {
            inst_retired,
            core_cycles,    
            ref_cycles,           
            llc_ref: llc_ref?,       
            llc_miss: llc_miss?,
            br_inst_ret: br_inst_ret?,   
            br_miss_ret: br_miss_ret?,
        } )
    }

    /// Start running all the counters 
    pub fn start(&mut self) -> Result<(), &'static str>{
        self.ref_cycles.start()?;
        self.core_cycles.start()?;
        self.inst_retired.start()?;        
        self.llc_ref.start()?;
        self.llc_miss.start()?;
        self.br_inst_ret.start()?;
        self.br_miss_ret.start()?;
        Ok(())
    }

    /// Stop the counters and return the counter values
    pub fn end(&mut self) -> Result<PMUResults, &'static str> {
        Ok( PMUResults {
            inst_retired: self.inst_retired.end()?,
            core_cycles: self.core_cycles.end()?, 
            ref_cycles: self.ref_cycles.end()?, 
            llc_ref: self.llc_ref.end()?, 
            llc_miss: self.llc_miss.end()?, 
            br_inst_ret: self.br_inst_ret.end()?, 
            br_miss_ret: self.br_miss_ret.end()?
        } )
    }
}