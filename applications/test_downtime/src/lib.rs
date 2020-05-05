#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate spin;
extern crate task;
extern crate spawn;
extern crate scheduler;
extern crate rendezvous;
extern crate async_channel;
extern crate apic;
extern crate runqueue;
extern crate window;
extern crate framebuffer;
extern crate framebuffer_drawer;
extern crate color;
extern crate shapes;
extern crate hpet;


use alloc::{
    vec::Vec,
    string::String,
};

use core::sync::atomic::{AtomicUsize, Ordering};
use getopts::{Matches, Options};
use spin::Once;
use color::Color;
use shapes::Coord;
use window::Window;
use spin::Mutex;
use spawn::new_task_builder;
use hpet::get_hpet;

const NANO_TO_FEMTO: u64 = 1_000_000;

pub struct PassStruct {
    pub count : usize,
    pub user : usize,
}

macro_rules! CPU_ID {
	() => (apic::get_my_apic_id())
}

lazy_static! {

    /// The structure to hold the list of all faults so far occured in the system
    pub static ref DOWNTIME_SEND_LOCK: Mutex<PassStruct> = Mutex::new(PassStruct{count : 0, user : 0});
    pub static ref DOWNTIME_RECEIVE_LOCK: Mutex<PassStruct> = Mutex::new(PassStruct{count : 0, user : 0});
}

pub fn set_watch_task() -> (){
    let arg_val = 0;

    // setup a task to send coordinates
    let taskref1  = new_task_builder(watch_task, arg_val)
        .name(String::from("watch task"))
        .pin_on_core(pick_child_core())
        .spawn_restartable()
        .expect("Couldn't start the watch task");

    // setup a task to receive responses
    let taskref2  = new_task_builder(send_task, arg_val)
        .name(String::from("send task"))
        .pin_on_core(pick_child_core())
        .spawn_restartable()
        .expect("Couldn't start the send task");

}

fn send_task(_arg_val: usize) -> Result<(), &'static str>{

    // This task send cordates from 100 to 300 and keeps on repeating
    let mut start_hpet: u64;
    let mut end_hpet: u64;

    warn!("TEST MSG : SEND STARTED");

    #[cfg(use_crate_replacement)]
    warn!("test_multiple(): Crate replacement will occur at the fault");

    let mut send_val: usize = 0;
    let mut count = 100;
    loop {

        let mut element = DOWNTIME_SEND_LOCK.lock();
        if(element.user == 0){
            element.user = 1;
            element.count = count;
            count = count + 1;
            if(count == 300){
                count = 100;
            }
        }
    }
    Ok(())
}

fn watch_task(_arg_val: usize) -> Result<(), &'static str>{


    let mut vec = Vec::with_capacity(200);

    let mut start_hpet: u64;
    let mut end_hpet: u64;

    start_hpet = get_hpet().as_ref().unwrap().get_counter();
    warn!("TEST MSG : RECEIVE STARTED");
    let mut send_val: usize = 0;
    let mut count = 1;

    let mut aggregted_count = 0;
    let mut aggregated_total = 0;
    loop {

        let mut element = DOWNTIME_RECEIVE_LOCK.lock();
        if(element.user == 0){
            element.user = 1;
            end_hpet = get_hpet().as_ref().unwrap().get_counter();
            element.count = count;

            // We actually measure the time between two responses - inverse throughoutput
            vec.push(end_hpet - start_hpet);

            start_hpet = end_hpet;
            count = count + 1;

            // When count = 199 an iteration is completed
            if(count == 200){
                warn!("TEST MSG : TEST COMPLETED");
                let (new_count, new_total) = print_stats(vec.clone(),aggregted_count.clone(),aggregated_total.clone());
                aggregted_count = new_count;
                aggregated_total = new_total;
                // loop{

                // }
                vec.clear();
                count = 1;
            }
        }
    }
    Ok(())
}

fn print_stats(vec: Vec<u64>, aggregted_count: u64, aggregted_total :u64) ->(u64,u64){
    let mut count1 = 0;
    for (i,val) in vec.iter().enumerate() {

        // Get the normal operation time by averaging 128 samples from the middle of operations
        if i > 31 && i < 96 {
            count1 = count1 + val;
        }
        if i > 127 && i < 192 {
            count1 = count1 + val;
        }

        // The value when restart happens is at vec[101]
        // if(i>97 && i < 104) {
        //     debug!("{} : {} : {}", i, val, hpet_2_ns(*val));
        // }
        //debug!("{} : {}", i, val);
    }

    count1 = count1 / 128;
    debug!("Ticks in normal : {} ", count1);
    debug!("time in normal {}", hpet_2_ns(count1));
    debug!("Ticks due to restart {}", vec[101]);
    debug!("Time due to restart {}", hpet_2_ns(vec[101]));

    let count = aggregted_count + 1;
    let total = aggregted_total + vec[101];

    if count == 16 {
        let total = total / 16;
        debug!("Average restart ticks = {}", total);
        debug!("Average restart time = {}", hpet_2_ns(total));
    }
    (count,total)
}


pub fn pick_child_core() -> u8 {
	// try with current core -1
	let child_core: u8 = (CPU_ID!() as u8).saturating_sub(1);
	if nr_tasks_in_rq(child_core) == Some(1) {return child_core;}

	// if failed, try from the last to the first
	for child_core in (0..apic::core_count() as u8).rev() {
		if nr_tasks_in_rq(child_core) == Some(1) {return child_core;}
	}
	debug!("WARNING : Cannot pick a child core because cores are busy");
	debug!("WARNING : Selecting current core");
	return child_core;
}

fn nr_tasks_in_rq(core: u8) -> Option<usize> {
	match runqueue::get_runqueue(core).map(|rq| rq.read()) {
		Some(rq) => { Some(rq.iter().count()) }
		_ => { None }
	}
}

fn hpet_2_ns(hpet: u64) -> u64 {
	let hpet_period = get_hpet().as_ref().unwrap().counter_period_femtoseconds();
	hpet * hpet_period as u64 / NANO_TO_FEMTO
}

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("w", "window", "measures downtime of window");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };
    
    let arg_val = 0;
    if matches.opt_present("w"){

        set_watch_task();
        
        let taskref1  = new_task_builder(fault_graphics_task, arg_val)
            .name(String::from("fault_graphics_task"))
            .pin_on_core(2)
            .spawn_restartable()
            .expect("Couldn't start the fault_graphics_task");

        taskref1.join().expect("Task 1 join failed");
    }

    return 0; 
}

fn fault_graphics_task(_arg_val: usize) -> Result<(), &'static str>{

    // This task draws a circile on the cordinates received from the watch thread
    {
        debug!("Starting the fault task");
        let window_wrap =  Window::new(
            Coord::new(500,50),
            500,
            500,
            color::WHITE,
        );

        let mut window = window_wrap.expect("Window creation failed");

        loop {

            let mut send_val:isize = 0;

            // Get a pair of cordinates
            loop{
                let mut element = DOWNTIME_SEND_LOCK.lock();
                if(element.user == 1){
                    element.user = 0;
                    send_val = element.count as isize;
                    break;
                }
            }

            //let mut fb_mut = window.framebuffer_mut();

            let color = Color::new(0x239B56);

            framebuffer_drawer::draw_circle(
                &mut window.framebuffer_mut(),
                Coord::new(send_val,send_val),
                50,
                framebuffer::Pixel::weight_blend(
                    color::RED.into(), 
                    color.into(),
                    10.0,
                ),
            );

            //window.display();
            window.render(Some(shapes::Rectangle{
                top_left : Coord::new(send_val - 50,send_val - 50), 
                bottom_right : Coord::new(send_val + 50,send_val + 50)
            }))?;

            // Send the response of success
            loop{
                let mut element = DOWNTIME_RECEIVE_LOCK.lock();
                if(element.user == 1){
                    element.user = 0;
                    break;
                }
            }

        }
    }

    loop {

    }
    Ok(())
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: test_channel OPTION ARG
Provides a selection of different tests for channel-based communication.";
