#![no_std]


extern crate alloc;
#[macro_use] extern crate log;
extern crate mpmc;
extern crate spawn;
extern crate pmu_x86;
extern crate tsc;
#[macro_use] extern crate lazy_static;
extern crate apic;

use alloc::vec::Vec;
use alloc::string::String;
use mpmc::Queue;
use spawn::KernelTaskBuilder;

const CYCLES: u16 = 2000;

lazy_static! {
    static ref Q12: Queue<u8> = Queue::with_capacity(CYCLES as usize *2);
    static ref Q21: Queue<u8> = Queue::with_capacity(CYCLES as usize *2);
}

pub fn test_ipc() -> Result<u8, &'static str> {

    let t0 = KernelTaskBuilder::new(measure_fuction_call_time, ()).pin_on_core(2).spawn()?;

    // let t1 = KernelTaskBuilder::new(consumer, ()).pin_on_core(2).spawn()?;
    // let t2 = KernelTaskBuilder::new(producer, ()).pin_on_core(4).spawn()?;

    t0.join()?;
    // t1.join()?;
    // t2.join()?;

    Ok(0)
}

fn measure_fuction_call_time(_: ())-> Result<[usize; 10000], &'static str> {
    const iterations: usize = 10000;
    let mut a: [usize; iterations] = [0; iterations];

    pmu_x86::init();
    let mut counter = pmu_x86::Counter::new(pmu_x86::EventType::UnhaltedThreadCycles)?;
    for i in 0..iterations {
        a[i] = getpid(i);
    }

    let cycles = counter.get_count_since_start()?;
    let _ = counter.end();

    error!("{} function call in {} cpu cycles", iterations, cycles);

    Ok(a)
}

fn getpid(i: usize) -> usize{
    return i;
}

//t1
fn consumer(_: ()) {
    //warm up
    for i in 0..CYCLES as u8 {
        while Q21.pop().is_none() {}
        let _ = Q12.push(i%255); 
    }

    // actual
    for i in 0..CYCLES as u8 {
        while Q21.pop().is_none() {}
        let _ = Q12.push(i%255);
    }
}

// t2
fn producer(_: ()) -> Result<(), &'static str> {
    pmu_x86::init();
    let mut counter = pmu_x86::Counter::new(pmu_x86::EventType::UnhaltedThreadCycles)?;
    
    //warm up
    for i in 0..CYCLES as u8{
        let _ = Q21.push(i%255);
        while Q12.pop().is_none() {}

    }

    counter.start();
    // actual
    for i in 0..CYCLES as u8{
        let _ = Q21.push(i%255);
        while Q12.pop().is_none() {}
    }

    let cycles = counter.get_count_since_start()?;
    let _ = counter.end();

    let tsc_freq = tsc::get_tsc_frequency()?;

    error!("{} cycles of IPC in {} cpu cycles", CYCLES, cycles);
    error!("TSC frequency is: {}", tsc_freq);

    // error!("Completed 1000 cycles!");

    Ok(())

}