//! This crate contains the `RunQueue` structure, which is essentially a list of Tasks
//! that it used for scheduling purposes.
//! 

#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate task;

use alloc::VecDeque;
use irq_safety::RwLockIrqSafe;
use atomic_linked_list::atomic_map::AtomicMap;
use task::TaskRef;



lazy_static! {
    /// There is one runqueue per core, each core only accesses its own private runqueue
    /// and allows the scheduler to select a task from that runqueue to schedule in.
    static ref RUNQUEUES: AtomicMap<u8, RwLockIrqSafe<RunQueue>> = AtomicMap::new();
}


/// A list of references to `Task`s (`TaskRef`s) 
/// that is used to store the `Task`s that are runnable on a given core. 
pub struct RunQueue {
    core: u8,
    queue: VecDeque<TaskRef>,
}

impl RunQueue {
    /// Creates a new `RunQueue` for the given core, which is an `apic_id`.
    pub fn init(which_core: u8) -> Result<(), &'static str> {
        trace!("Created runqueue for core {}", which_core);
        let new_rq = RwLockIrqSafe::new(RunQueue {
            core: which_core,
            queue: VecDeque::new(),
        });
        if RUNQUEUES.insert(which_core, new_rq).is_some() {
            error!("BUG: RunQueue::init(): runqueue already exists for core {}!", which_core);
            Err("runqueue already exists for this core")
        }
        else {
            // there shouldn't already be a RunQueue for this core
            Ok(())
        }
    }

    /// Creates a new `RunQueue` for the given core, which is an `apic_id`.
    pub fn get_runqueue(which_core: u8) -> Option<&'static RwLockIrqSafe<RunQueue>> {
        RUNQUEUES.get(&which_core)
    }


    /// Returns the "least busy" core, which is currently very simple, based on runqueue size.
    pub fn get_least_busy_core() -> Option<u8> {
        Self::get_least_busy_runqueue().map(|rq| rq.read().core)
    }


    /// Returns the `RunQueue` for the "least busy" core.
    /// See [`get_least_busy_core()`](#method.get_least_busy_core)
    pub fn get_least_busy_runqueue() -> Option<&'static RwLockIrqSafe<RunQueue>> {
        let mut min_rq: Option<(&'static RwLockIrqSafe<RunQueue>, usize)> = None;

        for (_, rq) in RUNQUEUES.iter() {
            let rq_size = rq.read().queue.len();

            if let Some(min) = min_rq {
                if rq_size < min.1 {
                    min_rq = Some((rq, rq_size));
                }
            }
            else {
                min_rq = Some((rq, rq_size));
            }
        }

        min_rq.map(|m| m.0)
    }

    /// Chooses the "least busy" core's runqueue (based on simple runqueue-size-based load balancing)
    /// and adds the given `Task` reference to that core's runqueue.
    pub fn add_task_to_any_runqueue(task: TaskRef) -> Result<(), &'static str> {
        let rq = RunQueue::get_least_busy_runqueue()
            .or_else(|| RUNQUEUES.iter().next().map(|r| r.1))
            .ok_or("couldn't find any runqueues to add the task to!")?;

        rq.write().add_task(task)
    }

    /// Convenience method that adds the given `Task` reference to given core's runqueue.
    pub fn add_task_to_specific_runqueue(which_core: u8, task: TaskRef) -> Result<(), &'static str> {
        RunQueue::get_runqueue(which_core)
            .ok_or("Couldn't get RunQueue for the given core")?
            .write()
            .add_task(task.clone())
    }

    /// Adds a `TaskRef` to this RunQueue.
    pub fn add_task(&mut self, task: TaskRef) -> Result<(), &'static str> {
        debug!("Adding task to runqueue {}, {:?}", self.core, task);
        self.queue.push_back(task);
        Ok(())
    }


    /// Retrieves the `TaskRef` in this `RunQueue` at the specified `index`.
    /// Index 0 is the front of the RunQueue.
    pub fn get(&self, index: usize) -> Option<&TaskRef> {
        self.queue.get(index)
    }

    /// Removes a `TaskRef` from this RunQueue.
    pub fn remove_task(&mut self, task: &TaskRef) -> Result<(), &'static str> {
        debug!("Removing task from runqueue {}, {:?}", self.core, task);
        // debug!("BEFORE RUNQUEUE {}: {:?}", self.core, self.queue);
        self.queue.retain(|x| x != task);
        // debug!("AFTER RUNQUEUE {}: {:?}", self.core, self.queue);
        Ok(())
    }

    /// Removes a `TaskRef` from all `RunQueue`s that exist on the entire system.
    pub fn remove_task_from_all(task: &TaskRef) -> Result<(), &'static str> {
        for (_core, rq) in RUNQUEUES.iter() {
            rq.write().remove_task(task)?;
        }
        Ok(())
    }

    /// Moves the `TaskRef` at the given index into this `RunQueue` to the end (back) of this `RunQueue`,
    /// and returns a cloned reference to that `TaskRef`.
    pub fn move_to_end(&mut self, index: usize) -> Option<TaskRef> {
        self.queue.remove(index).map(|taskref| {
            self.queue.push_back(taskref.clone());
            taskref
        })
    }


    /// Returns an iterator over all `TaskRef`s in this `RunQueue`.
    pub fn iter(&self) -> alloc::vec_deque::Iter<TaskRef> {
        self.queue.iter()
    }
}
