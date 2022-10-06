#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate mod_mgmt;
// #[macro_use] extern crate lazy_static;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate task;
extern crate hpet;

extern crate runqueue_round_robin;
extern crate runqueue_priority;

use core::ops::Deref;
use alloc::sync::Arc;
use mod_mgmt::CrateNamespace;
// use lazy_static::lazy::Lazy;
use irq_safety::RwLockIrqSafe;
use atomic_linked_list::atomic_map::AtomicMap;
// use task::TaskRef;


/// This function is used for live evolution from a round robin scheduler to a priority scheduler. 
/// It first extracts the taskrefs from the round robin Runqueue,
/// then converts them into priority taskrefs and places them on the priority Runqueue.
pub fn prio_sched(_old_namespace: &Arc<CrateNamespace>, _new_namespace: &CrateNamespace) -> Result<(), &'static str> {

    // just debugging info
    #[cfg(not(loscd_eval))]
    {
        warn!("prio_sched(): at the top.");
        let rq_rr_crate = CrateNamespace::get_crate_starting_with(_old_namespace, "runqueue_round_robin")
            .map(|(_crate_name, crate_ref, _ns)| crate_ref)
            .ok_or("Couldn't get runqueue_round_robin crate from old namespace")?;
        let krate = rq_rr_crate.lock_as_ref();
        for sec in krate.sections.values() {
            if sec.name.contains("RUNQUEUES") {
                debug!("Section {}\n\ttype: {:?}\n\tvaddr: {:#X}\n\tsize: {}\n", sec.name, sec.typ, sec.start_address(), sec.size());
            }
        }
        warn!("REPLACING LAZY_STATIC RUNQUEUES...");
    }

    #[cfg(loscd_eval)]
    let hpet = hpet::get_hpet().ok_or("couldn't get HPET timer")?;
    #[cfg(loscd_eval)]
    let hpet_start_state_transfer = hpet.get_counter();

    // __lazy_static_create!(RQEMPTY, AtomicMap<u8, RwLockIrqSafe<runqueue_round_robin::RunQueue>>);
    // lazy_static! { static ref RQEMPTY: AtomicMap<u8, RwLockIrqSafe<runqueue_round_robin::RunQueue>> = AtomicMap::new(); }
    let rq_ptr = &runqueue_round_robin::RUNQUEUES as *const _ as usize;
    let once_rq = core::mem::replace(
        unsafe { &mut *(rq_ptr as *mut AtomicMap<u8, RwLockIrqSafe<runqueue_round_robin::RunQueue>>) }, 
        AtomicMap::new()
    );

    #[cfg(not(loscd_eval))]
    warn!("obtained ownership of runqueues:");
    for (core, rq) in once_rq.iter() {
        #[cfg(not(loscd_eval))]
        warn!("\tRunqueue on core {:?}: {:?}", core, rq);
        runqueue_priority::RunQueue::init(*core)?;
        for t in rq.read().iter() {
            runqueue_priority::RunQueue::add_task_to_specific_runqueue(*core, t.deref().clone())?;
        }
    }

    #[cfg(loscd_eval)] {
        let hpet_end_state_transfer = hpet.get_counter();
        warn!("Measured time in units of HPET ticks:
            state transfer, {}
            ",
            hpet_end_state_transfer - hpet_start_state_transfer,
        );
    }

    core::mem::drop(once_rq);

    #[cfg(not(loscd_eval))]
    warn!("REPLACED LAZY_STATIC RUNQUEUES...");


    Ok(())
}



//////////////////////////////////////////////////////////
///////////// Generic trait redefinitions ////////////////
//////////////////////////////////////////////////////////

/// Just like core::convert::From
trait MyFrom<T>: Sized {
    fn from(t: T) -> Self;
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
