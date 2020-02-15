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
use task::TaskRef;


/// Used for the evolution from a round robin scheduler to a priority scheduler
pub fn prio_sched(old_namespace: &Arc<CrateNamespace>, new_namespace: &CrateNamespace) -> Result<(), &'static str> {

    warn!("prio_sched(): at the top.");
    // Since we don't currently support updating a running application's code with the new object code, 
    // we just hack together a solution for the terminal, since it's the only long-running app. 
    // We only need to fix up this dependency:
    //     runqueue_round_robin::RunQueue::remove_task  -->  runqueue_priority::RunQueue::remove_task
    let old_section = old_namespace.get_symbol_starting_with("runqueue_round_robin::RunQueue::remove_task::").upgrade()
        .ok_or_else(|| {
            error!("prio_sched(): Couldn't find symbol in old namespace: \"runqueue_round_robin::RunQueue::remove_task::\"");
            "prio_sched(): Couldn't find symbol in old namespace: \"runqueue_round_robin::RunQueue::remove_task::\""
        })?;
    let new_section = new_namespace.get_symbol_starting_with("runqueue_priority::RunQueue::remove_task::").upgrade()
        .ok_or_else(|| {
            error!("prio_sched(): Couldn't find symbol in old namespace: \"runqueue_priority::RunQueue::remove_task::\"");
            "prio_sched(): Couldn't find symbol in old namespace: \"runqueue_priority::RunQueue::remove_task::\""
        })?;
    
    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("couldn't get kernel MMI ref")?;
    warn!("prio_sched(): calling rewrite_section_dependents...");
    CrateNamespace::rewrite_section_dependents(&old_section, &new_section, &kernel_mmi_ref)?;
    warn!("prio_sched(): finished rewrite_section_dependents.");


    // Extract the taskrefs from the round robin Runqueue, 
    // then convert them into priority taskrefs and place them on the priority Runqueue.
    let rq_rr_crate = CrateNamespace::get_crate_starting_with(old_namespace, "runqueue_round_robin")
        .map(|(_crate_name, crate_ref, _ns)| crate_ref)
        .ok_or("Couldn't get runqueue_round_robin crate from old namespace")?;
    
    let krate = rq_rr_crate.lock_as_ref();
    for sec_ref in krate.sections.values() {
        let sec = sec_ref.read();
        if sec.name.contains("RUNQUEUES") {
            warn!("Section {}\n\ttype: {:?}\n\tvaddr: {:#X}\n\tsize: {}\n", sec.name, sec.typ, sec.start_address(), sec.size());
        }
    }

    warn!("REPLACING LAZY_STATIC RUNQUEUES...");

    #[cfg(loscd_eval)]
    let hpet_ref = hpet::get_hpet();
    #[cfg(loscd_eval)]
    let hpet = hpet_ref.as_ref().ok_or("couldn't get HPET timer")?;
    #[cfg(loscd_eval)]
    let hpet_start_state_transfer = hpet.get_counter();

    // __lazy_static_create!(RQEMPTY, AtomicMap<u8, RwLockIrqSafe<runqueue_round_robin::RunQueue>>);
    // lazy_static! { static ref RQEMPTY: AtomicMap<u8, RwLockIrqSafe<runqueue_round_robin::RunQueue>> = AtomicMap::new(); }
    let rq_ptr = runqueue_round_robin::RUNQUEUES.deref() as *const _ as usize;
    let once_rq = core::mem::replace(
        unsafe { &mut *(rq_ptr as *mut AtomicMap<u8, RwLockIrqSafe<runqueue_round_robin::RunQueue>>) }, 
        AtomicMap::new()
    );

    #[cfg(not(loscd_eval))]
    warn!("obtained ownership of runqueues:");
    for (core, rq) in once_rq.iter() {
        #[cfg(not(loscd_eval))]
        warn!("\t{:?}: {:?}", core, rq);
        runqueue_priority::RunQueue::init(*core)?;
        for t in rq.read().iter() {
            let rrtref: &_RoundRobinTaskRef = unsafe { core::mem::transmute(t) };
            runqueue_priority::RunQueue::add_task_to_specific_runqueue(*core, rrtref._taskref.clone())?;
        }
    }

    #[cfg(loscd_eval)] {
        let hpet_end_state_transfer = hpet.get_counter();
        warn!("
            state transfer, {}
            ",
            hpet_end_state_transfer - hpet_start_state_transfer,
        );
    }

    core::mem::drop(once_rq);

    warn!("REPLACED LAZY_STATIC RUNQUEUES...");


    Ok(())
}


struct _RoundRobinTaskRef{
    _taskref: TaskRef,
    _context_switches: u32,
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
