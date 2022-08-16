#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
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
extern crate unified_channel;


use alloc::{
    vec::Vec,
    string::String,
};

use getopts::Options;
use color::Color;
use shapes::Coord;
use window::Window;
use spin::Mutex;
use spawn::new_task_builder;
use hpet::get_hpet;
use unified_channel::{StringSender, StringReceiver};

const NANO_TO_FEMTO: u64 = 1_000_000;

pub struct PassStruct {
    pub count : usize,
    pub user : usize,
}

macro_rules! CPU_ID {
	() => (apic::get_my_apic_id())
}

// ------------------------- Window fault injection section -------------------------------------------

/// The structure to hold the list of all faults so far occured in the system
pub static DOWNTIME_SEND_LOCK: Mutex<PassStruct> = Mutex::new(PassStruct{count : 0, user : 0});
pub static DOWNTIME_RECEIVE_LOCK: Mutex<PassStruct> = Mutex::new(PassStruct{count : 0, user : 0});

/// Setup two tasks to send cordinates and receive a response
pub fn set_graphics_measuring_task() -> (){
    let arg_val = 0;

    // setup a task to send coordinates
    let _taskref1  = new_task_builder(graphics_measuring_task, arg_val)
        .name(String::from("watch task"))
        .pin_on_core(pick_child_core())
        .spawn_restartable(None)
        .expect("Couldn't start the watch task");

    // setup a task to receive responses
    let _taskref2  = new_task_builder(graphics_send_task, arg_val)
        .name(String::from("send task"))
        .pin_on_core(pick_child_core())
        .spawn_restartable(None)
        .expect("Couldn't start the send task");

}

/// This task send cordates from 100 to 300 and keeps on repeating
fn graphics_send_task(_arg_val: usize) -> Result<(), &'static str>{

    warn!("TEST MSG : SEND STARTED");

    #[cfg(use_crate_replacement)]
    warn!("test_multiple(): Crate replacement will occur at the fault");

    let mut count = 100;
    loop {

        let mut element = DOWNTIME_SEND_LOCK.lock();
        if element.user == 0 {
            element.user = 1;
            element.count = count;
            count = count + 1;
            if count == 300 {
                count = 100;
            }
        }
    }
}

/// This task measures the time difference between two received responses
fn graphics_measuring_task(_arg_val: usize) -> Result<(), &'static str>{

    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
    let mut vec = Vec::with_capacity(200);

    let mut start_hpet: u64;
    let mut end_hpet: u64;

    start_hpet = hpet.get_counter();
    warn!("TEST MSG : RECEIVE STARTED");
    let mut count = 1;

    // Aggregation of dowtime from multiple runs.
    let mut aggregted_count = 0;
    let mut aggregated_total = 0;
    loop {

        let mut element = DOWNTIME_RECEIVE_LOCK.lock();
        if element.user == 0 {
            element.user = 1;
            end_hpet = hpet.get_counter();
            element.count = count;

            // We actually measure the time between two responses - inverse throughoutput
            vec.push(end_hpet - start_hpet);

            start_hpet = end_hpet;
            count = count + 1;

            if count == 200 {
                warn!("TEST MSG : TEST COMPLETED");
                let (new_count, new_total) = graphics_print_stats(vec.clone(),aggregted_count.clone(),aggregated_total.clone());
                aggregted_count = new_count;
                aggregated_total = new_total;
                // loop{

                // }
                vec.clear();
                count = 1;
            }
        }
    }
}

/// Utility function to print the downtime and normal operation time
fn graphics_print_stats(vec: Vec<u64>, aggregted_count: u64, aggregted_total :u64) ->(u64,u64){
    let slice = &vec[10..90];
    let sum: u64  = Iterator::sum(slice.iter());
    let count1 = sum / (slice.len() as u64);

    debug!("Ticks in normal : {} ", count1);
    debug!("time in normal {}", hpet_2_ns(count1));
    debug!("Ticks due to restart {}", vec[101]); // Since we injected the fault at this specific cordinate
    debug!("Some random RT values {} {} {}", vec[40], vec[60], vec[80]); // To double check values are within range  
    debug!("Time due to restart {}", hpet_2_ns(vec[101]));

    // After 16 restarts we print the average of downtime.
    let count = aggregted_count + 1;
    let total = aggregted_total + vec[101];

    if count == 16 {
        let total = total / 16;
        debug!("Average restart ticks = {}", total);
        debug!("Average restart time = {}", hpet_2_ns(total));
    }
    (count,total)
}

/// The restartable task which runs into fault when it tries to draw a circle at (200,200) location. 
/// This task is not suppose to exit (It fails due to the injected fault)
fn fault_graphics_task(_arg_val: usize) -> Result<(), &'static str>{

    // This task draws a circile on the cordinates received from the watch thread
    {
        // debug!("Starting the fault task");
        let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
        let start_hpet = hpet.get_counter();
        let window_wrap =  Window::new(
            Coord::new(500,50),
            500,
            500,
            color::WHITE,
        );

        let mut window = window_wrap.expect("Window creation failed");
        let end_hpet = hpet.get_counter();
        let mut first_round_val = 0;
        let mut first_time = true;

        loop {

            let send_val: isize;

            // Get a pair of cordinates
            loop{
                let mut element = DOWNTIME_SEND_LOCK.lock();
                if element.user == 1 {
                    element.user = 0;
                    send_val = element.count as isize;
                    break;
                }
            }

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

            window.render(Some(shapes::Rectangle{
                top_left : Coord::new(send_val - 50,send_val - 50), 
                bottom_right : Coord::new(send_val + 50,send_val + 50)
            }))?;

            // Send the response of success
            loop{
                let mut element = DOWNTIME_RECEIVE_LOCK.lock();
                if element.user == 1 {
                    element.user = 0;
                    break;
                }
            }

            if first_time {
                first_round_val = hpet.get_counter();
                first_time = false;
            }

            // When calculating average we ignore last few values. 
            // So this don't interfere with measurements
            if send_val == 295 {
                debug!("Window creation took : {}",end_hpet - start_hpet);
                debug!("Window creation remaining {}",first_round_val - end_hpet);
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------
// ------------------------- IPC fault injection section -------------------------------------------

/// This task just loops back the received message. 
/// It is not suppose to exit during normal operation.
/// It faults due to an injected fault in IPC routines
fn ipc_fault_task((sender,receiver) : (StringSender, StringReceiver)) -> Result<(), &'static str>{

    // sending the initial message of staring
    let start_msg = format!("99999999");
    sender.send(start_msg)?;

    let mut i = 0;
    loop {
        let msg = receiver.receive()?;
        // let hpet = get_hpet().ok_or("couldn't get HPET timer")?;
        // let value = hpet().get_counter();
        // warn!("test_multiple(): Received {} at {}", msg, value);
        sender.send(msg)?;
        i = i + 1;
    }
}

// Setup the channels and measuring task
fn set_ipc_watch_task() -> (StringSender, StringReceiver){

    // create two channels to pass messages
    let (sender, receiver) = unified_channel::new_string_channel(2);
    let (sender_reply, receiver_reply) = unified_channel::new_string_channel(2);

    // Create the sending task
    let _taskref1  = new_task_builder(ipc_watch_task, (sender, receiver_reply))
        .name(String::from("watch task"))
        .pin_on_core(pick_child_core())
        .spawn()
        .expect("Couldn't start the watch task");

    (sender_reply, receiver)
}

// Measuring task. Measures the roundtrip and store the values.
// Detects disconnection and outputs downtime
fn ipc_watch_task((sender, receiver) : (StringSender, StringReceiver)) -> Result<(), &'static str>{

    #[cfg(not(use_async_channel))]
    warn!("test_multiple(): Entered receiver task! : using rendevouz channel");

    #[cfg(use_async_channel)]
    warn!("test_multiple(): Entered receiver task! : using async channel");

    #[cfg(use_crate_replacement)]
    warn!("test_multiple(): Crate replacement will occur at the fault");

    // Cyclic buffer to store the round trip times
    let mut array: [u64; 128] = [0; 128];
    let mut i = 0;
    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;

    // received the initial config message (we need this to prevent deadlock in task restarts)
    receiver.receive()?;

    loop {
        let msg = format!("{:08}", i);
        let msg_copy = format!("{:08}", i);

        let time_send = hpet.get_counter();
        // warn!("test_multiple(): Sender sending message {:08} {}", i, time_send);
        sender.send(msg)?;
        let msg_received;
        loop {
            if let Ok(msg) = receiver.receive() {
                msg_received = msg;
                break;
            }
        }
        let time_received = hpet.get_counter();

        // If the test occured correctly we update the round trip log
        if msg_copy == msg_received {
            array[i%128] = time_received - time_send;
        }

        // warn!("test_multiple(): Receiver got {:?}  ({:08}) {} {} {}", msg_received, i, time_send, time_received, time_received - time_send);

        // If an error occured we receive the restart message
        if msg_copy != msg_received {
            // We ignore faults immediatelty occuring after restart
            if i > 256 {
                // warn!("Sender restart noted");
                let fault_ticks = time_received - time_send;
                let average_ticks : u64 = array.iter().sum();
                let average_ticks = average_ticks / 128;

                warn!("Sender restart noted at {}
                        Fault : {} : {}
                        Average : {} : {} ", i,
                        fault_ticks, hpet_2_ns (fault_ticks),
                        average_ticks, hpet_2_ns (average_ticks));

                // for i in &array {
                //     debug!("{}", i);
                // }
                //loop{}
            } else {
                warn!("Ignoring error at {}", i);
            }
            i = 0;
        }
        i = i + 1;
    }
}

// -------------------------------------------------------------------------------------------------

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
    opts.optflag("a", "async", "measures downtime of IPC connection based on async channel");
    opts.optflag("r", "rendezvous", "measures downtime of IPC connection based on rendezvous channel");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };
    
    let arg_val = 0;

    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }

    #[cfg(all(not(downtime_eval), not(loscd_eval)))]
    {
        println!("Compile with downtime_eval and loscd_eval cfg options to run the tests");
    }

    if matches.opt_present("w"){

        set_graphics_measuring_task();
        
        let taskref1  = new_task_builder(fault_graphics_task, arg_val)
            .name(String::from("fault_graphics_task"))
            .pin_on_core(2)
            .spawn_restartable(None)
            .expect("Couldn't start the fault_graphics_task");

        taskref1.join().expect("Task 1 join failed");
    }

    if matches.opt_present("a") { 

        #[cfg(use_async_channel)] 
        {
            // Set the channel
            let (sender_reply, receiver) = set_ipc_watch_task();

            debug!("Channel set up completed");

            let taskref1  = new_task_builder(ipc_fault_task, (sender_reply, receiver))
                .name(String::from("fault_task"))
                .pin_on_core(pick_child_core())
                .spawn_restartable(None)
                .expect("Couldn't start the restartable task"); 

            taskref1.join().expect("Task 1 join failed");
        }

        #[cfg(not(use_async_channel))]
        println!("compile with use_async_channel to enable async channel");
    }

    if matches.opt_present("r") { 

       #[cfg(not(use_async_channel))] 
        {
            // Set the channel
            let (sender_reply, receiver) = set_ipc_watch_task();

            debug!("Channel set up completed");

            let taskref1  = new_task_builder(ipc_fault_task, (sender_reply, receiver))
                .name(String::from("fault_task"))
                .pin_on_core(pick_child_core())
                .spawn_restartable(None)
                .expect("Couldn't start the restartable task"); 

            taskref1.join().expect("Task 1 join failed");
        }

        #[cfg(use_async_channel)]
        println!("compile without use_async_channel to enable rendezvous channel");
    }

    return 0; 
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: test_channel OPTION ARG
Provides a selection of different tests for channel-based communication.";
