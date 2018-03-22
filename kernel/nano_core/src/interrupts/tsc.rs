use core::sync::atomic::{AtomicUsize, Ordering};
use interrupts::pit_clock;

static TSC_FREQUENCY: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub struct TscTicks(u64);

impl TscTicks {
    /// Converts ticks to nanoseconds. 
    /// Returns None if the TSC tick frequency is unavailable.
    pub fn to_ns(&self) -> Option<u64> {
         let freq = get_tsc_frequency();
         if freq == 0 {
             None
         }
         else {
            Some( (self.0 * 1000000000) / freq )
         }
    }

    /// Checked subtraction. Computes `self - other`, 
    /// returning `None` if underflow occurred.
    pub fn sub(&self, other: &TscTicks) -> Option<TscTicks> {
        let checked_sub = self.0.checked_sub(other.0);
        checked_sub.map( |tt| TscTicks(tt) )
    }
    
    /// Checked addition. Computes `self + other`, 
    /// returning `None` if overflow occurred.
    pub fn add(&self, other: &TscTicks) -> Option<TscTicks> {
        let checked_add = self.0.checked_add(other.0);
        checked_add.map( |tt| TscTicks(tt) )
    }

    /// Get the inner value, the number of ticks.
    pub fn into(self) -> u64 {
        self.0
    }

    pub const fn default() -> TscTicks {
        TscTicks(0)
    }
}


/// Initializes the TSC, which only 
pub fn init() -> Result<(), &'static str> {
    let start = tsc_ticks();
    try!(pit_clock::pit_wait(10000)); // wait 10000 us (10 ms)
    let end = tsc_ticks(); 

    let diff = try!(end.sub(&start).ok_or("couldn't subtract end-start TSC tick values"));
    let tsc_freq = diff.into() * 100; // multiplied by 100 because we measured a 10ms interval
    info!("TSC frequency calculated by PIT is: {}", tsc_freq);
    set_tsc_frequency(tsc_freq);
    Ok(())
}


/// Returns the current number of ticks from the TSC, i.e., `rdtsc`. 
pub fn tsc_ticks() -> TscTicks {
    let mask: u64 = 0xFFFF_FFFF;
    let high: u64;
    let low: u64;
    // SAFE: just using rdtsc asm instructions
    unsafe {
        // lfence is a cheaper fence instruction than cpuid
        asm!("lfence; rdtsc"
            : "={edx}"(high), "={eax}"(low)
            :
            : "rdx", "rax"
            : "volatile"
        );
    }
    
    TscTicks( ((mask&high)<<32) | (mask&low) )
}

/// Returns the frequency of the TSC for the system, 
/// currently measured using the PIT clock for calibration.
/// A frequency of 0 means it hasn't yet been calibrated.
pub fn get_tsc_frequency() -> u64 {
    TSC_FREQUENCY.load(Ordering::SeqCst) as u64
}

#[doc(hidden)]
pub fn set_tsc_frequency(new_tsc_freq: u64) {
    TSC_FREQUENCY.store(new_tsc_freq as usize, Ordering::Release);
}



////////////////////////////////////////////////////////////////
/////////////////////// OLD CODE BELOW /////////////////////////
////////////////////////////////////////////////////////////////


//const INVARIANT_TSC_AVAILABILITY_REGISTER: u32 = 0x80000007;
//const TSC_CALIBRATION_LOOPS: u64 = 10;
//const BASE_TO_TSC_MULTIPLIER_INDEX: u32 = 0xce; 
//const PROCESSOR_BASE_FREQUENCY_CPUID_ADDRESS: u32 = 0x15;

/*
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
//check to for invariant tsc
pub fn invariant_tsc()->bool{

    let result: u32;
    unsafe {
        asm!("mov eax, $1
            cpuid" 
            : "={edx}"(result) //output
            :"r"(INVARIANT_TSC_AVAILABILITY_REGISTER) //input
            :"rax", "rdx" //clobbered registers
            :"intel", "volatile") //options: uses intel syntax for convenience, and volatile to prevent reordering memory operations
    }
    let modified = result;
    trace!("{}",modified);
    //(result>>8) == 1
    false 

}
*/
// /*two tsc functions, one for starting count and one for end:
// cpuid forces in order instruction. In start function, cpuid is placed 
// before RDTSC and after RDTSCP in end function so cpuid instruction not added inside of counted cycles    
// (Page 16 Intel "How to Benchmark Code Execution" manual) */
// #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
// pub fn get_start_tsc()->u64{

//     //RDTSC opcode puts lower half of 64 bit value in register eax and upper half in edx
//     let low_order: u64;
//     let high_order: u64;

//     //clearing eax and calling cpuid ensures out of order instruction does not happen because of cpuid's use of registers
//     unsafe {
//         asm!("cpuid
//             RDTSC" 
//             :"={eax}"(low_order), "={edx}"(high_order) 
//             ://no input 
//             : "rax","rbx","rcx", "rdx"
//             :"intel", "volatile")
//     }
    
//     high_order<<32 | low_order

// }

// #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
// pub fn get_end_tsc()->u64{

//     //RDTSC opcode puts lower half of 64 bit value in register eax and upper half in edx
//     let low_order: u32;
//     let high_order: u32;

//     //clearing eax and calling cpuid ensures out of order instruction does not happen because of cpuid's use of registers
//     unsafe {
//         asm!("RDTSCP
//             mov $0, eax
//             mov $1, edx
//             cpuid"
//             :"=r"(low_order), "=r"(high_order) 
//             ://no input
//             : "rax","rcx", "rdx"
//             :"intel", "volatile")
//     }
    
//     //it seems like if if not reading from a register, output of inline assembly must be u32(match the size of memory being read from)
//     (high_order as u64)<<32 | (low_order as u64)
    
    

// } 

/*
//uses PIT timer to count number of TSC cycles in 1 second to determine frequency
pub fn calibrate_tsc()->u64{


    
    let mut total_cycles: u64 = 0;
    let mut start_pit: usize;
    let mut start_tsc: u64;
    let mut end_tsc: u64;
    let frequency: u64;
    
    start_pit = pit_clock::PIT_TICKS.load(Ordering::SeqCst);
    start_tsc = get_start_tsc();
    while(pit_clock::PIT_TICKS.load(Ordering::SeqCst) < start_pit+1000){}    
    end_tsc = get_end_tsc();
    
    frequency =  end_tsc - start_tsc;

    trace!("TSC frequency calculated using PIT in TSC function is: {}", frequency);
    
    frequency

}
*/

/*
//gets ratio of nonturbo clock to tsc frequency
pub fn read_msr(msr_address:u32)->u32{

    //low and high order values to be read from registers
    let low_msr: u32;
    let high_msr: u32;

    unsafe {
        asm!("mov eax, $2
            rdmsr" 
            : "={eax}"(low_msr), "={edx}"(high_msr)  //output
            :"r"(msr_address) //input
            :"rax", "rdx", "rcx" //clobbered registers
            :"intel", "volatile") //options: uses intel syntax for convenience, and volatile to prevent reordering memory operations
    }
    
    //concatenate to get full 64 bit value
    //let msr_info:u64 = high_msr<<32 | low_msr;

    //clear bottom 16 bits and shift 8 (possibly wrong, need to come back to this)
    
    trace!("{}, msr information at index ", low_msr);

    low_msr

}
*/

/*
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
//instructions from section 18.18.3 in vol 3 Intel 64 and IA-32 Software Developers manual (pg 804)
pub fn calculate_tsc_frequency()->u64{

    let register_b: u64;
    let register_a: u64;
    let register_c: u64;
    let frequency: u64;

    unsafe {
        asm!("mov eax, 0x15
            cpuid" 
            : "={ebx}"(register_b),"={eax}"(register_a)  //output
            :"r"(PROCESSOR_BASE_FREQUENCY_CPUID_ADDRESS) //input
            :"rax", "rdx", "rbx" //clobbered registers
            :"intel", "volatile") //options: uses intel syntax for convenience, and volatile to prevent reordering memory operations
    }

    frequency = (24000000*register_b);
    trace!("Calculated frequency using cpuid data is {}", frequency);
    //let non_turbo_ratio =(read_msr(BASE_TO_TSC_MULTIPLIER_INDEX) & 0xff00) >> 8;
    //100000 * non_turbo_ratio
    frequency

}
*/