#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
// extern crate memory;
extern crate mod_mgmt;
// #[macro_use] extern crate lazy_static;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate task;

extern crate runqueue_round_robin;
extern crate runqueue_priority;

use core::ops::Deref;
use mod_mgmt::CrateNamespace;
// use lazy_static::lazy::Lazy;
use irq_safety::RwLockIrqSafe;
use atomic_linked_list::atomic_map::AtomicMap;
use task::TaskRef;


/// Used for the evolution from a round robin scheduler to a priority scheduler
pub fn prio_sched(old_namespace: &CrateNamespace, new_namespace: &CrateNamespace) -> Result<(), &'static str> {

    warn!("IN PRIO SCHED STATE TRANSFER FUNCTION");
    // Extract the taskrefs from the round robin Runqueue, 
    // then convert them into priority taskrefs and place them on the priority Runqueue.
    let rq_rr_crate = old_namespace.get_crate_starting_with("runqueue_round_robin").map(|(_crate_name, crate_ref)| crate_ref)
        .ok_or("Couldn't get runqueue_round_robin crate from old namespace")?;
    
    let krate = rq_rr_crate.lock_as_ref();
    for sec_ref in krate.sections.values() {
        let sec = sec_ref.lock();
        if sec.name.contains("RUNQUEUES") {
            warn!("Section {}\n\ttype: {:?}\n\tvaddr: {:#X}\n\tsize: {}\n", sec.name, sec.typ, sec.virt_addr(), sec.size);
        }
    }

    warn!("REPLACING LAZY_STATIC RUNQUEUES...");

    // __lazy_static_create!(RQEMPTY, AtomicMap<u8, RwLockIrqSafe<runqueue_round_robin::RunQueue>>);
    // lazy_static! { static ref RQEMPTY: AtomicMap<u8, RwLockIrqSafe<runqueue_round_robin::RunQueue>> = AtomicMap::new(); }
    let rq_ptr = runqueue_round_robin::RUNQUEUES.deref() as *const _ as usize;
    let once_rq = core::mem::replace(
        unsafe { &mut *(rq_ptr as *mut AtomicMap<u8, RwLockIrqSafe<runqueue_round_robin::RunQueue>>) }, 
        AtomicMap::new()
    );

    warn!("obtained ownership of runqueues:");
    for (core, rq) in once_rq.iter() {
        warn!("\t{:?}: {:?}", core, rq);
        runqueue_priority::RunQueue::init(*core)?;
        for t in rq.read().iter() {
            let rrtref: &_RoundRobinTaskRef = unsafe { core::mem::transmute(t) };
            runqueue_priority::RunQueue::add_task_to_specific_runqueue(*core, rrtref.taskref.clone())?;
        }
    }

    core::mem::drop(once_rq);

    warn!("REPLACED LAZY_STATIC RUNQUEUES...");


    Ok(())
}


struct _RoundRobinTaskRef{
    taskref: TaskRef,
    context_switches: u32,
}


//////////////////////////////////////////////////////////
///////////// Generic trait redefinitions ////////////////
//////////////////////////////////////////////////////////

/// Just like core::convert::From
trait MyFrom<T>: Sized {
    fn from(T) -> Self;
}

/// Just like core::convert::Into
trait MyInto<T>: Sized {
    fn into(self) -> T;
}

impl<T, U> MyInto<U> for T where U: MyFrom<T> {
    fn into(self) -> U {
        U::from(self)
    }
}
