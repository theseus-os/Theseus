#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate mod_mgmt;

use mod_mgmt::CrateNamespace;


/// Used for the evolution from a round robin scheduler to a priority scheduler
pub fn prio_sched(old_namespace: &CrateNamespace, new_namespace: &CrateNamespace) -> Result<(), &'static str> {

    // extract the taskrefs from the RR runqueue

    // convert them into priority taskrefs and place them on the prio runqueue
    warn!("IN PRIO SCHED STATE TRANSFER FUNCTION");
    
    Ok(())
}