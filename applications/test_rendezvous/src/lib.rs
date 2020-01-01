#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate task;
extern crate spawn;
extern crate scheduler;
extern crate rendezvous;
extern crate apic;

use alloc::{
    vec::Vec,
    string::String,
    // sync::Arc,
};
use spawn::KernelTaskBuilder;
// use rendezvous::{Sender, Receiver};




#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    const DEFAULT_ITERATIONS: usize = 500;

    let res = match _args.get(0).map(|s| &**s) {
        // Some("-c") => test_contention(),
        Some(num) => rmain(num.parse().unwrap_or(DEFAULT_ITERATIONS)),
        _ => test_multiple(DEFAULT_ITERATIONS),
    };
    match res {
        Ok(_) => 0,
        Err(e) => {
            error!("Error: {}", e); 
            -1
        }
    }
    
}

fn rmain(iterations: usize) -> Result<(), &'static str> {
    for _i in 0..iterations {
        test_oneshot()?;
    }
    Ok(())
}

/// A simple test that spawns a sender & receiver task to send a single message
fn test_oneshot() -> Result<(), &'static str> {
    let my_cpu = apic::get_my_apic_id().ok_or("couldn't get my APIC ID")?;

    let (sender, receiver) = rendezvous::new_channel();

    let t1 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("Entered sender task!");
        sender.send("hello")?;
        Ok(())
    }, ())
        .name(String::from("sender_task"))
        // .pin_on_core(my_cpu)
        .block()
        .spawn()?;

    let t2 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("Entered receiver task!");
        let msg = receiver.receive()?;
        warn!("Receiver got msg: {:?}", msg);
        Ok(())
    }, ())
        .name(String::from("receiver_task"))
        // .pin_on_core(my_cpu)
        .block()
        .spawn()?;


    warn!("Finished spawning the sender and receiver tasks");

    t2.unblock(); t1.unblock();

    t1.join()?;
    t2.join()?;
    warn!("Joined the sender and receiver tasks.");
    
    Ok(())
}



/// A simple test that spawns a sender & receiver task to send `iterations` messages.
fn test_multiple(iterations: usize) -> Result<(), &'static str> {
    let my_cpu = apic::get_my_apic_id().ok_or("couldn't get my APIC ID")?;

    let (sender, receiver) = rendezvous::new_channel();

    let t1 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("Entered sender task!");
        for i in 0..iterations {
            sender.send(format!("Message {:03}", i))?;
            warn!("Sender sent message {:03}", i);
        }
        Ok(())
    }, ())
        .name(String::from("sender_task"))
        // .pin_on_core(my_cpu)
        .block()
        .spawn()?;

    let t2 = KernelTaskBuilder::new(|_: ()| -> Result<(), &'static str> {
        warn!("Entered receiver task!");
        for i in 0..iterations {
            let msg = receiver.receive()?;
            warn!("Receiver got {:?}  ({:03})", msg, i);
        }
        Ok(())
    }, ())
        .name(String::from("receiver_task"))
        // .pin_on_core(my_cpu)
        .block()
        .spawn()?;


    warn!("Finished spawning the sender and receiver tasks");

    t2.unblock(); t1.unblock();

    t1.join()?;
    t2.join()?;
    warn!("Joined the sender and receiver tasks.");
    
    Ok(())
}