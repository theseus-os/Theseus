//! This crate contains the `Task` structure for supporting multithreading, 
//! and the associated code for dealing with tasks.
//! 
//! To create new `Task`s, use the [`spawn`](../spawn/index.html) crate.
//! 
//! # Examples
//! How to wait for a `Task` to complete (using `join()`) and get its exit value.
//! ```
//! spawn::join(&taskref)); // taskref is the task that we're waiting on
//! let locked_task = taskref.read();
//! if let Some(exit_result) = locked_task.get_exit_value() {
//!     match exit_result {
//!         Ok(exit_value) => {
//!             // here: the task ran to completion successfully, so it has an exit value.
//!             // we know the return type of this task is `isize`,
//!             // so we need to downcast it from Any to isize.
//!             let val: Option<&isize> = exit_value.downcast_ref::<isize>();
//!             warn!("task returned exit value: {:?}", val);
//!         }
//!         Err(kill_reason) => {
//!             // here: the task exited prematurely, e.g., it was killed for some reason.
//!             warn!("task was killed, reason: {:?}", kill_reason);
//!         }
//!     }
//! }
//! ```
//! 

#![no_std]
#![feature(alloc)]
#![feature(asm, naked_functions)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate spin;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate memory;
extern crate tss;
extern crate apic;
extern crate mod_mgmt;
extern crate panic_info;

#[cfg(not(target_feature = "sse2"))]
extern crate context_switch;
#[cfg(target_feature = "sse2")]
extern crate context_switch_sse; 


use core::fmt;
use core::sync::atomic::{Ordering, AtomicUsize, AtomicBool, spin_loop_hint};
use core::any::Any;
use alloc::String;
use alloc::VecDeque;
use alloc::boxed::Box;
use alloc::arc::Arc;

use irq_safety::{MutexIrqSafe, RwLockIrqSafe, RwLockIrqSafeReadGuard, RwLockIrqSafeWriteGuard, interrupts_enabled};
use memory::{PageTable, Stack, MemoryManagementInfo, VirtualAddress};
use atomic_linked_list::atomic_map::AtomicMap;
use apic::get_my_apic_id;
use tss::tss_set_rsp0;
use mod_mgmt::metadata::StrongCrateRef;
use panic_info::PanicInfo;


#[cfg(not(target_feature = "sse2"))]
use context_switch::context_switch;
#[cfg(target_feature = "sse2")]
use context_switch_sse::context_switch;



/// The signature of the callback function that can hook into receiving a panic. 
pub type PanicHandler = Box<Fn(&PanicInfo)>;



lazy_static! {
    /// The id of the currently executing `Task`, per-core.
    pub static ref CURRENT_TASKS: AtomicMap<u8, usize> = AtomicMap::new();
}

lazy_static! {
    /// Used to ensure that task switches are done atomically on each core
    pub static ref TASK_SWITCH_LOCKS: AtomicMap<u8, AtomicBool> = AtomicMap::new();
}

lazy_static! {
    /// The list of all Tasks in the system.
    pub static ref TASKLIST: AtomicMap<usize, TaskRef> = AtomicMap::new();
}

lazy_static! {
    /// There is one runqueue per core, each core can only access its own private runqueue
    /// and select a task from that runqueue to schedule in.
    static ref RUNQUEUES: AtomicMap<u8, RwLockIrqSafe<RunQueue>> = AtomicMap::new();
}



/// Get the id of the currently running Task on a specific core
pub fn get_current_task_id(apic_id: u8) -> Option<usize> {
    CURRENT_TASKS.get(&apic_id).cloned()
}

/// Get the id of the currently running Task on this core.
pub fn get_my_current_task_id() -> Option<usize> {
    get_my_apic_id().and_then(|id| {
        get_current_task_id(id)
    })
}

/// returns a shared reference to the current `Task` running on this core.
pub fn get_my_current_task() -> Option<&'static TaskRef> {
    get_my_current_task_id().and_then(|id| {
        TASKLIST.get(&id)
    })
}

/// returns a shared reference to the `Task` specified by the given `task_id`
pub fn get_task(task_id: usize) -> Option<&'static TaskRef> {
    TASKLIST.get(&task_id)
}


/// Sets the panic handler function for the current `Task`
pub fn set_my_panic_handler(handler: PanicHandler) -> Result<(), &'static str> {
    get_my_current_task()
        .ok_or("couldn't get_my_current_task")
        .map(|taskref| {
            taskref.0.write().set_panic_handler(handler)
        })
}



/// The list of possible reasons that a given `Task` was killed prematurely.
#[derive(Debug)]
pub enum KillReason {
    /// The user or another task requested that this `Task` be killed. 
    /// For example, the user pressed `Ctrl + C` on the shell window that started a `Task`.
    Requested,
    /// A Rust-level panic occurred while running this `Task`
    Panic(PanicInfo),
    /// A non-language-level problem, such as a Page Fault or some other machine exception.
    /// The number of the exception is included, e.g., 15 (0xE) for a Page Fault.
    Exception(u8),
}


#[derive(Debug)]
/// The list of ways that a Task can exit, including possible return values and conditions.
pub enum ExitValue {
    /// The Task ran to completion and returned the enclosed `Any` value.
    /// The caller of this type should know what type this Task returned,
    /// and should therefore be able to downcast it appropriately.
    Completed(Box<Any>),
    /// The Task did NOT run to completion, and was instead killed.
    /// The reason for it being killed is enclosed. 
    Killed(KillReason),
}


#[derive(Debug)]
pub enum RunState {
    /// in the midst of setting up the task
    Initing,
    /// able to be scheduled in, but not necessarily currently running. 
    /// To check whether it is currently running, use [`is_running`](#method.is_running)
    Runnable,
    /// blocked on something, like I/O or a wait event
    Blocked,
    /// The `Task` has exited and can no longer be run,
    /// either by running to completion or being killed. 
    Exited(ExitValue),
    /// This `Task` had already exited and now its ExitValue has been taken;
    /// its exit value can only be taken once, and consumed by another `Task`.
    /// This `Task` is now useless, and can be deleted and removed from the Task list.
    Reaped,
}


/// A structure that contains contextual information for a thread of execution. 
pub struct Task {
    /// the unique id of this Task.
    pub id: usize,
    /// the simple name of this Task
    pub name: String,
    /// Which cpu core the Task is currently running on.
    /// `None` if not currently running.
    pub running_on_cpu: Option<u8>,
    /// the runnability status of this task, basically whether it's allowed to be scheduled in.
    pub runstate: RunState,
    /// the saved stack pointer value, used for task switching.
    pub saved_sp: usize,
    /// memory management details: page tables, mappings, allocators, etc.
    /// Wrapped in an Arc & Mutex because it's shared between all other tasks in the same address space
    pub mmi: Option<Arc<MutexIrqSafe<MemoryManagementInfo>>>, 
    /// the kernelspace stack.  Wrapped in Option<> so we can initialize it to None.
    pub kstack: Option<Stack>,
    /// the userspace stack.  Wrapped in Option<> so we can initialize it to None.
    pub ustack: Option<Stack>,
    /// for special behavior of new userspace task
    pub new_userspace_entry_addr: Option<VirtualAddress>, 
    /// Whether or not this task is pinned to a certain core
    /// The idle tasks (like idle_task) are always pinned to their respective cores
    pub pinned_core: Option<u8>,
    /// Whether this Task is an idle task, the task that runs by default when no other task is running.
    /// There exists one idle task per core.
    pub is_an_idle_task: bool,
    /// For application `Task`s, the [`LoadedCrate`](../mod_mgmt/metadata/struct.LoadedCrate.html)
    /// that contains the backing memory regions and sections for running this `Task`'s object file 
    pub app_crate: Option<StrongCrateRef>,
    /// The function that will be called when this `Task` panics
    pub panic_handler: Option<PanicHandler>,
}

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{Task \"{}\" ({}), running_on_cpu: {:?}, runstate: {:?}, pinned: {:?}}}", 
               self.name, self.id, self.running_on_cpu, self.runstate, self.pinned_core)
    }
}

impl Task {
    /// creates a new Task structure and initializes it to be non-Runnable.
    pub fn new() -> Task {
        /// The counter of task IDs
        static TASKID_COUNTER: AtomicUsize = AtomicUsize::new(0);

        // we should re-use old task IDs again, instead of simply blindly counting up
        // TODO FIXME: or use random values to avoid state spill
        let task_id = TASKID_COUNTER.fetch_add(1, Ordering::Acquire);
        
        Task {
            id: task_id,
            runstate: RunState::Initing,
            running_on_cpu: None,
            saved_sp: 0,
            name: format!("task{}", task_id),
            kstack: None,
            ustack: None,
            mmi: None,
            new_userspace_entry_addr: None,
            pinned_core: None,
            is_an_idle_task: false,
            app_crate: None,
            panic_handler: None,
        }
    }

    /// returns true if this Task is currently running on any cpu.
    pub fn is_running(&self) -> bool {
        self.running_on_cpu.is_some()
    }

    /// Returns true if this `Task` is Runnable, i.e., able to be scheduled in.
    /// # Note
    /// This does *NOT* mean that this `Task` is actually currently running, just that it is *able* to be run.
    pub fn is_runnable(&self) -> bool {
        match self.runstate {
            RunState::Runnable => true,
            _ => false,
        }
    }

    /// Returns true if this is an application `Task`.
    pub fn is_application(&self) -> bool {
        self.app_crate.is_some()
    }


    /// Registers a function or closure that will be called if this `Task` panics.
    pub fn set_panic_handler(&mut self, callback: PanicHandler) {
        self.panic_handler = Some(callback);
    }

    /// Takes ownership of this `Task`'s `PanicHandler` closure/function if one exists,
    /// and returns it so it can be invoked without holding this `Task`'s `RwLock`.
    /// After invoking this, the `Task`'s `panic_handler` will be `None`.
    pub fn take_panic_handler(&mut self) -> Option<PanicHandler> {
        self.panic_handler.take()
    }


    /// Returns a reference to the exit value of this `Task`, 
    /// if its runstate is `RunState::Exited`. 
    /// Unlike [`take_exit_value`](#method.take_exit_value), this does not consume the exit value.
    pub fn get_exit_value(&self) -> Option<&ExitValue> {
        if let RunState::Exited(ref val) = self.runstate {
            Some(val)
        } else {
            None
        }
    }

    /// Takes ownership of this `Task`'s exit value and returns it,
    /// if and only if this `Task` was in the `Exited` runstate.
    /// After invoking this, the `Task`'s runstate will be `Reaped`.
    pub fn take_exit_value(&mut self) -> Option<ExitValue> {
        match self.runstate {
            RunState::Exited(_) => { }
            _ => return None, 
        }

        let exited = core::mem::replace(&mut self.runstate, RunState::Reaped);
        if let RunState::Exited(exit_value) = exited {
            Some(exit_value)
        } 
        else {
            None
        }
    }

    /// Switches from the current (`self`)  to the given `next` Task
    /// no locks need to be held to call this, but interrupts (later, preemption) should be disabled
    pub fn task_switch(&mut self, next: &mut Task, apic_id: u8) {
        // debug!("task_switch [0]: (AP {}) prev {:?}, next {:?}", apic_id, self, next);
        
        let my_task_switch_lock: &AtomicBool = match TASK_SWITCH_LOCKS.get(&apic_id) {
            Some(csl) => csl,
            _ => {
                error!("BUG: task_switch(): no task switch lock present for AP {}, skipping task switch!", apic_id);
                return;
            } 
        };
        
        // acquire this core's task switch lock
        // TODO: add timeout
        while my_task_switch_lock.compare_and_swap(false, true, Ordering::SeqCst) {
            spin_loop_hint();
        }

        // debug!("task_switch [1], testing runstates.");
        if !next.is_runnable() {
            error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was not Runnable! Current: {:?}, Next: {:?}", self, next);
            my_task_switch_lock.store(false, Ordering::SeqCst);
            return;
        }
        if next.is_running() {
            error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was already running on AP {}!\nCurrent: {:?} Next: {:?}", apic_id, self, next);
            my_task_switch_lock.store(false, Ordering::SeqCst);
            return;
        }
        if let Some(pc) = next.pinned_core {
            if pc != apic_id {
                error!("BUG: Skipping context_Switch due to scheduler bug: chosen 'next' Task was pinned to AP {:?} but scheduled on AP {}!\nCurrent: {:?}, Next: {:?}", next.pinned_core, apic_id, self, next);
                my_task_switch_lock.store(false, Ordering::SeqCst);
                return;
            }
        }
         

        // update runstates
        self.running_on_cpu = None; // no longer running
        next.running_on_cpu = Some(apic_id); // now running on this core


        // change the privilege stack (RSP0) in the TSS
        // TODO: we can safely skip setting the TSS RSP0 when switching to kernel threads, i.e., when next is not a userspace task
        {
            let next_kstack = next.kstack.as_ref().expect("BUG: task_switch(): error: next task's kstack was None!");
            let new_tss_rsp0 = next_kstack.bottom() + (next_kstack.size() / 2); // the middle half of the stack
            if tss_set_rsp0(new_tss_rsp0).is_ok() { 
                // debug!("task_switch [2]: new_tss_rsp = {:#X}", new_tss_rsp0);
            }
            else {
                error!("task_switch(): failed to set AP {} TSS RSP0, aborting task switch!", apic_id);
                my_task_switch_lock.store(false, Ordering::SeqCst);
                return;
            }
        }

        // We now do the page table switching here, so we can use our higher-level PageTable abstractions
        {
            let prev_mmi = self.mmi.as_ref().expect("task_switch: couldn't get prev task's MMI!");
            let next_mmi = next.mmi.as_ref().expect("task_switch: couldn't get next task's MMI!");
            

            if Arc::ptr_eq(prev_mmi, next_mmi) {
                // do nothing because we're not changing address spaces
                // debug!("task_switch [3]: prev_mmi is the same as next_mmi!");
            }
            else {
                // time to change to a different address space and switch the page tables!

                let mut prev_mmi_locked = prev_mmi.lock();
                let mut next_mmi_locked = next_mmi.lock();
                // debug!("task_switch [3]: switching tables! From {} {:?} to {} {:?}", 
                //         self.name, prev_mmi_locked.page_table, next.name, next_mmi_locked.page_table);
                

                let new_active_table = {
                    // prev_table must be an ActivePageTable, and next_table must be an InactivePageTable
                    match &mut prev_mmi_locked.page_table {
                        &mut PageTable::Active(ref mut active_table) => {
                            active_table.switch(&next_mmi_locked.page_table)
                        }
                        _ => {
                            panic!("task_switch(): prev_table must be an ActivePageTable!");
                        }
                    }
                };
                
                // since we're no longer changing the prev page table to be inactive, just leave it be,
                // and only change the next task's page table to active 
                // (it was either active already, or it was previously inactive (and now active) if it was the first time it had been run)
                next_mmi_locked.set_page_table(PageTable::Active(new_active_table)); 

            }
        }
       
        // update the current task to `next`
        CURRENT_TASKS.insert(apic_id, next.id); 

        // release this core's task switch lock
        my_task_switch_lock.store(false, Ordering::SeqCst);

        unsafe {
            // debug!("task_switch [4]: prev sp: {:#X}, next sp: {:#X}", self.saved_sp, next.saved_sp);
            
            // because context_switch must be a naked function, we cannot directly pass it parameters
            // instead, we must pass our 2 parameters in RDI and RSI respectively
            asm!("mov rdi, $0; \
                  mov rsi, $1;" 
                : : "r"(&mut self.saved_sp as *mut usize), "r"(next.saved_sp)
                : "memory" : "intel", "volatile"
            );
            context_switch();
        }

    }
}

impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{{{}}}", self.name, self.id)
    }
}


/// A shareable, cloneable reference to a `Task` that exposes more methods
/// for task management, and accesses the enclosed `Task` by locking it. 
/// 
/// The `TaskRef` type is necessary because in many places across Theseus,
/// a reference to a Task is used
/// For example, runqueues, task lists, task spawning, task management, etc. 
/// 
/// Essentially a newtype wrapper around `Arc<[Lock]<Task>>` 
/// where `[Lock]` is some mutex-like locking type.
/// Currently, `[Lock]` is a `RwLockIrqSafe`.
#[derive(Debug, Clone)]
pub struct TaskRef(Arc<RwLockIrqSafe<Task>>);

impl TaskRef {
    /// Creates a new `TaskRef` that wraps the given `Task`.
    pub fn new(task: Task) -> TaskRef {
        TaskRef(Arc::new(RwLockIrqSafe::new(task)))
    }

    /// Waits until the given `task` has finished executing, 
    /// i.e., blocks until its runstate is `RunState::Exited`.
    /// Returns `Ok()` when the given `task` is actually exited,
    /// and `Err()` if there is a problem or interruption while waiting for it to exit. 
    /// 
    /// # Note
    /// * You cannot call `join()` on the current thread, because a thread cannot wait for itself to finish running. 
    ///   This will result in an `Err()` being immediately returned.
    /// * You cannot call `join()` with interrupts disabled, because it will result in permanent deadlock
    ///   (well, this is only true if the requested `task` is running on the same cpu...  but good enough for now).
    pub fn join(&self) -> Result<(), &'static str> {
        let curr_task = get_my_current_task().ok_or("join(): failed to check what current task is")?;
        if Arc::ptr_eq(&self.0, &curr_task.0) {
            return Err("BUG: cannot call join() on yourself (the current task).");
        }

        if !interrupts_enabled() {
            return Err("BUG: cannot call join() with interrupts disabled; it will cause deadlock.")
        }
        
        // First, wait for this Task to be marked as Exited (no longer runnable).
        loop {
            if let RunState::Exited(_) = self.0.read().runstate {
                break;
            }
        }

        // Then, wait for it to actually stop running on any CPU core.
        loop {
            let t = self.0.read();
            if !t.is_running() {
                return Ok(());
            }
        }
    }


    /// The internal routine that actually exits or kills a Task.
    fn internal_exit(&self, val: ExitValue) -> Result<(), &'static str> {
        let mut task = self.0.write();
        if let RunState::Exited(_) = task.runstate {
            return Err("task was already exited! (did not overwrite its existing exit value)");
        }
        task.runstate = RunState::Exited(val);
        Ok(())
    }

    /// Call this function to indicate that this task has successfully ran to completion,
    /// and that it has returned the given `exit_value`.
    /// 
    /// # Locking / Deadlock
    /// This method obtains a writeable lock on the underlying Task in order to mutate its state.
    /// 
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set.     
    /// * Returns `Err` if this `Task` was already exited, and does not overwrite the existing exit status. 
    ///  
    /// # Note 
    /// The `Task` will not be halted immediately -- 
    /// it will finish running its current timeslice, and then never be run again.
    pub fn exit(&self, exit_value: Box<Any>) -> Result<(), &'static str> {
        self.internal_exit(ExitValue::Completed(exit_value))
    }


    /// Kills this `Task` (not a clean exit) without allowing it to run to completion.
    /// The given `KillReason` indicates why it was killed.
    /// 
    /// # Locking / Deadlock
    /// This method obtains a writeable lock on the underlying Task in order to mutate its state.
    /// 
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set to the given `KillReason`.     
    /// * Returns `Err` if this `Task` was already exited, and does not overwrite the existing exit status. 
    /// 
    /// # Note 
    /// The `Task` will not be halted immediately -- 
    /// it will finish running its current timeslice, and then never be run again.
    pub fn kill(&self, reason: KillReason) -> Result<(), &'static str> {
        self.internal_exit(ExitValue::Killed(reason))
    }


    /// Obtains the lock on the underlying `Task` in a read-only, blocking fashion.
    /// This is okay because we want to allow any other part of the OS to read 
    /// the details of the `Task` struct.
    pub fn read(&self) -> RwLockIrqSafeReadGuard<Task> {
        self.0.read()
    }

    /// Registers a function or closure that will be called if this `Task` panics.
    /// # Locking / Deadlock
    /// Obtains a write lock on the enclosed `Task` in order to mutate its state.
    pub fn set_panic_handler(&self, callback: PanicHandler) {
        self.0.write().set_panic_handler(callback)
    }

    /// Takes ownership of this `Task`'s `PanicHandler` closure/function if one exists,
    /// and returns it so it can be invoked without holding this `Task`'s `RwLock`.
    /// After invoking this, the `Task`'s `panic_handler` will be `None`.
    /// # Locking / Deadlock
    /// Obtains a write lock on the enclosed `Task` in order to mutate its state.
    pub fn take_panic_handler(&self) -> Option<PanicHandler> {
        self.0.write().take_panic_handler()
    }

    /// Takes ownership of this `Task`'s exit value and returns it,
    /// if and only if this `Task` was in the `Exited` runstate.
    /// After invoking this, the `Task`'s runstate will be `Reaped`.
    /// # Locking / Deadlock
    /// Obtains a write lock on the enclosed `Task` in order to mutate its state.
    pub fn take_exit_value(&self) -> Option<ExitValue> {
        self.0.write().take_exit_value()
    }

    /// Obtains the lock on the underlying `Task` in a writeable, blocking fashion.
    #[deprecated]
    pub fn write(&self) -> RwLockIrqSafeWriteGuard<Task> {
        self.0.write()
    }
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
        self.queue.retain(|x| !Arc::ptr_eq(&x.0, &task.0));
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
