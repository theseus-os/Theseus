#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate spawn;
extern crate rendezvous;
extern crate apic;
extern crate runqueue;

use alloc::{
    vec::Vec,
    string::String,
};

use getopts::Options;
use spawn::new_task_builder;

macro_rules! CPU_ID {
	() => (apic::get_my_apic_id())
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


pub fn main(args: Vec<String>) -> isize {

    let mut opts = Options::new();
	opts.optflag("", "s1", "s1 fault");
	opts.optflag("", "s2", "s2 fault");
	opts.optflag("", "s3", "s3 fault");
	opts.optflag("", "s4", "s4 fault");
	opts.optflag("", "s5", "s5 fault");
	opts.optflag("", "s6", "s6 fault");
	opts.optflag("", "s7", "s7 fault");
	opts.optflag("", "r1", "r1 fault");
	opts.optflag("", "r2", "r2 fault");
	opts.optflag("", "r3", "r3 fault");
	opts.optflag("", "r4", "r4 fault");
	opts.optflag("", "r5", "r5 fault");
	opts.optflag("", "r6", "r6 fault");
	opts.optflag("", "s7", "s7 fault");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            return -1; 
        }
    };
    
    let arg_val = 0;

    let mut fault_num;

    if matches.opt_present("s1") {
        fault_num = 1;
    } else if matches.opt_present("s2") {
        fault_num = 12;
    } else if matches.opt_present("s3") {
        fault_num = 2;
    } else if matches.opt_present("s4") {
        fault_num = 3;
    } else if matches.opt_present("s5") {
        fault_num = 5;
    } else if matches.opt_present("s6") {
        fault_num = 11;
    } else if matches.opt_present("s7") {
        fault_num = 13;
    } else if matches.opt_present("r1") {
        fault_num = 4;
    } else if matches.opt_present("r2") {
        fault_num = 6;
    } else if matches.opt_present("r3") {
        fault_num = 7;
    } else if matches.opt_present("r4") {
        fault_num = 8;
    } else if matches.opt_present("r5") {
        fault_num = 9;
    } else if matches.opt_present("r6") {
        fault_num = 10;
    } else {
        fault_num = 0;
    }

    // A simple IPC loop to use in Minix fault injection comparsion

	// Notify the type of fault to rendezvous channel
        rendezvous::set_fault_item(fault_num);
        // create two channels to pass messages
        let (sender, receiver) = rendezvous::new_channel::<String>();
        let taskref1  = new_task_builder(ipc_send_task, sender)
                .name(String::from("send_task"))
                .pin_on_core(pick_child_core())
                .spawn_restartable()
                .expect("Couldn't start the restartable task");
                
        let taskref2  = new_task_builder(ipc_receive_task, (receiver, fault_num))
                .name(String::from("receive_task"))
                .pin_on_core(pick_child_core())
                .spawn_restartable()
                .expect("Couldn't start the restartable task");

    return 0; 
}

fn ipc_send_task(sender: rendezvous::Sender<String>) -> Result<(), &'static str>{

    debug!("send task started");

    let mut i = 0;
    loop {
        let msg = format!("{:08}", i);
        sender.send(msg)?;
        i = i + 1;
    }

    

    Ok(())
}

fn ipc_receive_task((receiver, test) : (rendezvous::Receiver<String>, usize)) -> Result<(), &'static str>{

    debug!("receive task started");
    loop {
        let msg_final = format!("{:08}", 10);

        let msg = receiver.receive()?;

        if msg_final == msg {
            
            debug!("TEST RESULT {} receive task successfully completed", test);
            loop{

            }
        }
    }

    Ok(())
}
