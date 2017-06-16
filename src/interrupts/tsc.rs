#![feature(asm)]
use interrupts::pit_clock;
use core::sync::atomic::{AtomicUsize, Ordering};

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



//returns the frequency of the tsc for the system
pub fn get_tsc_frequency()->u64{

    pit_clock::TSC_FREQUENCY.load(Ordering::SeqCst) as u64
}

//takes a tsc cycle value and returns the number of nanoseconds it represents
pub fn tsc_cycles_to_time(cycles: u64)-> u64{
    
    cycles*1000000000/get_tsc_frequency()
    

}

/*two tsc functions, one for starting count and one for end:
cpuid forces in order instruction. In start function, cpuid is placed 
before RDTSC and after RDTSCP in end function so cpuid instruction not added inside of counted cycles    
(Page 16 Intel "How to Benchmark Code Execution" manual) */
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn get_start_tsc()->u64{

    //RDTSC opcode puts lower half of 64 bit value in register eax and upper half in edx
    let low_order: u64;
    let high_order: u64;

    //clearing eax and calling cpuid ensures out of order instruction does not happen because of cpuid's use of registers
    unsafe {
        asm!("cpuid
            RDTSC" 
            :"={eax}"(low_order), "={edx}"(high_order) 
            ://no input 
            : "rax","rbx","rcx", "rdx"
            :"intel", "volatile")
    }
    
    high_order<<32 | low_order

}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn get_end_tsc()->u64{

    //RDTSC opcode puts lower half of 64 bit value in register eax and upper half in edx
    let low_order: u32;
    let high_order: u32;

    //clearing eax and calling cpuid ensures out of order instruction does not happen because of cpuid's use of registers
    unsafe {
        asm!("RDTSCP
            mov $0, eax
            mov $1, edx
            cpuid"
            :"=r"(low_order), "=r"(high_order) 
            ://no input
            : "rax","rcx", "rdx"
            :"intel", "volatile")
    }
    
    //it seems like if if not reading from a register, output of inline assembly must be u32(match the size of memory being read from)
    (high_order as u64)<<32 | (low_order as u64)
    
    

} 

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