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
        Ok(PerformanceCounters {
            inst_retired:  Counter::new(EventType::InstructionsRetired)?,
            core_cycles: Counter::new(EventType::UnhaltedCoreCycles)?,    
            ref_cycles: Counter::new(EventType::UnhaltedReferenceCycles)?,           
            llc_ref: Counter::new(EventType::LastLevelCacheReferences)?,       
            llc_miss: Counter::new(EventType::LastLevelCacheMisses)?,
            br_inst_ret: Counter::new(EventType::BranchInstructionsRetired)?,   
            br_miss_ret: Counter::new(EventType::BranchMissesRetired)?,
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

    /// Stop the counters and return the counter values.
    /// The `PerformanceCounters` object is consumed since the counters are freed in this function
    /// and should not be accessed again.
    pub fn end(self) -> Result<PMUResults, &'static str> {
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
