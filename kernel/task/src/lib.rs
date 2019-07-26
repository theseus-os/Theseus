//! This crate contains the `Task` structure for supporting multithreading, 
//! and the associated code for dealing with tasks.
//! 
//! To create new `Task`s, use the [`spawn`](../spawn/index.html) crate.
//! 
//! # Examples
//! How to wait for a `Task` to complete (using `join()`) and get its exit value.
//! ```
//! taskref.join(); // taskref is the task that we're waiting on
//! if let Some(exit_result) = taskref.take_exit_value() {
//!     match exit_result {
//!         ExitValue::Completed(exit_value) => {
//!             // here: the task ran to completion successfully, so it has an exit value.
//!             // We should know the return type of this task, e.g., if `isize`,
//!             // we would need to downcast it from Any to isize.
//!             let val: Option<&isize> = exit_value.downcast_ref::<isize>();
//!             warn!("task returned exit value: {:?}", val);
//!         }
//!         ExitValue::Killed(kill_reason) => {
//!             // here: the task exited prematurely, e.g., it was killed for some reason.
//!             warn!("task was killed, reason: {:?}", kill_reason);
//!         }
//!     }
//! }
//! ```
//! 

#![no_std]
#![feature(asm, naked_functions)]
#![feature(panic_info_message)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate memory;
extern crate tss;
extern crate mod_mgmt;
extern crate context_switch;
extern crate environment;
extern crate root;
extern crate x86_64;
extern crate spin;
extern crate fs_node;
extern crate pause;



use core::fmt;
use core::sync::atomic::{Ordering, AtomicUsize, AtomicBool};
use core::any::Any;
use core::panic::PanicInfo;
use core::ops::Deref;
use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::String,
    sync::Arc,
};
use pause::spin_loop_hint;

use irq_safety::{MutexIrqSafe, MutexIrqSafeGuardRef, MutexIrqSafeGuardRefMut, interrupts_enabled};
use memory::{Stack, MappedPages, PageRange, EntryFlags, MemoryManagementInfo, VirtualAddress};
use atomic_linked_list::atomic_map::AtomicMap;
use tss::tss_set_rsp0;
use mod_mgmt::metadata::StrongCrateRef;
use environment::Environment;
use spin::Mutex;
use x86_64::registers::msr::{rdmsr, wrmsr, IA32_FS_BASE};
use fs_node::DirRef;


/// The signature of the callback function that can hook into receiving a panic. 
pub type PanicHandler = Box<dyn Fn(&PanicInfoOwned) + Send>;

/// Just like `core::panic::PanicInfo`, but with owned String types instead of &str references.
#[derive(Debug, Clone)]
pub struct PanicInfoOwned {
    pub msg:    String,
    pub file:   String,
    pub line:   u32, 
    pub column: u32,
}
impl fmt::Display for PanicInfoOwned {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{:?}:{}:{} -- {:?}", self.file, self.line, self.column, self.msg)
    }
}
impl PanicInfoOwned {
    pub fn from(info: &PanicInfo) -> PanicInfoOwned {
        let msg = info.message()
            .map(|m| format!("{}", m))
            .unwrap_or_else(|| String::new());
        let (file, line, column) = if let Some(loc) = info.location() {
            (String::from(loc.file()), loc.line(), loc.column())
        } else {
            (String::new(), 0, 0)
        };

        PanicInfoOwned { msg, file, line, column }
    }
}


lazy_static! {
    /// Used to ensure that task switches are done atomically on each core
    pub static ref TASK_SWITCH_LOCKS: AtomicMap<u8, AtomicBool> = AtomicMap::new();
}

lazy_static! {
    /// The list of all Tasks in the system.
    pub static ref TASKLIST: MutexIrqSafe<BTreeMap<usize, TaskRef>> = MutexIrqSafe::new(BTreeMap::new());
}


/// returns a shared reference to the `Task` specified by the given `task_id`
pub fn get_task(task_id: usize) -> Option<TaskRef> {
    TASKLIST.lock().get(&task_id).cloned()
}


/// Sets the panic handler function for the current `Task`
pub fn set_my_panic_handler(handler: PanicHandler) -> Result<(), &'static str> {
    get_my_current_task()
        .ok_or("couldn't get_my_current_task")
        .map(|taskref| {
            taskref.set_panic_handler(handler)
        })
}



/// The list of possible reasons that a given `Task` was killed prematurely.
#[derive(Debug)]
pub enum KillReason {
    /// The user or another task requested that this `Task` be killed. 
    /// For example, the user pressed `Ctrl + C` on the shell window that started a `Task`.
    Requested,
    /// A Rust-level panic occurred while running this `Task`
    Panic(PanicInfoOwned),
    /// A non-language-level problem, such as a Page Fault or some other machine exception.
    /// The number of the exception is included, e.g., 15 (0xE) for a Page Fault.
    Exception(u8),
}


/// The list of ways that a Task can exit, including possible return values and conditions.
#[derive(Debug)]
pub enum ExitValue {
    /// The Task ran to completion and returned the enclosed `Any` value.
    /// The caller of this type should know what type this Task returned,
    /// and should therefore be able to downcast it appropriately.
    Completed(Box<dyn Any + Send>),
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


#[cfg(runqueue_state_spill_evaluation)]
/// A callback that will be invoked to remove a specific task from a specific runqueue.
/// Should be initialized by the runqueue crate.
pub static RUNQUEUE_REMOVAL_FUNCTION: Once<fn(&TaskRef, u8) -> Result<(), &'static str>> = Once::new();


#[cfg(simd_personality)]
/// The supported levels of SIMD extensions that a `Task` can use.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SimdExt {
    /// AVX (and below) instructions and registers will be used.
    AVX,
    /// SSE instructions and registers will be used.
    SSE,
    /// The regular case: no SIMD instructions or registers of any kind will be used.
    None,
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
    
    #[cfg(runqueue_state_spill_evaluation)]
    /// The runqueue that this Task is on.
    pub on_runqueue: Option<u8>,
    
    /// the runnability status of this task, basically whether it's allowed to be scheduled in.
    pub runstate: RunState,
    /// the saved stack pointer value, used for task switching.
    pub saved_sp: usize,
    /// the virtual address of (a pointer to) the `TaskLocalData` struct, which refers back to this `Task` struct.
    task_local_data_ptr: VirtualAddress,
    /// Data that should be dropped after a task switch; for example, the previous Task's TaskLocalData.
    drop_after_task_switch: Option<Box<dyn Any + Send>>,
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
    /// The environment of the task, Wrapped in an Arc & Mutex because it is shared among child and parent tasks
    pub env: Arc<Mutex<Environment>>,

    #[cfg(simd_personality)]
    /// Whether this Task is SIMD enabled and what level of SIMD extensions it uses.
    pub simd: SimdExt,
}

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{Task \"{}\" ({}), running_on_cpu: {:?}, runstate: {:?}, pinned: {:?}}}", 
               self.name, self.id, self.running_on_cpu, self.runstate, self.pinned_core)
    }
}

impl Task {
    /// Creates a new Task structure and initializes it to be non-Runnable.
    /// # Note
    /// This does not run the task, schedule it in, or switch to it.
    pub fn new() -> Task {
        /// The counter of task IDs
        static TASKID_COUNTER: AtomicUsize = AtomicUsize::new(0);

        // we should re-use old task IDs again, instead of simply blindly counting up
        // TODO FIXME: or use random values to avoid state spill
        let task_id = TASKID_COUNTER.fetch_add(1, Ordering::Acquire);

        // TODO - change to option and initialize environment to none
        let env = Environment {
            working_dir: Arc::clone(root::get_root()), 
        };

        Task {
            id: task_id,
            runstate: RunState::Initing,
            running_on_cpu: None,
            
            #[cfg(runqueue_state_spill_evaluation)]
            on_runqueue: None,
            
            saved_sp: 0,
            task_local_data_ptr: VirtualAddress::zero(),
            drop_after_task_switch: None,
            name: format!("task{}", task_id),
            kstack: None,
            ustack: None,
            mmi: None,
            new_userspace_entry_addr: None,
            pinned_core: None,
            is_an_idle_task: false,
            app_crate: None,
            panic_handler: None,
            env: Arc::new(Mutex::new(env)),

            #[cfg(simd_personality)]
            simd: SimdExt::None,
        }
    }

    fn set_env(&mut self, new_env:Arc<Mutex<Environment>>) {
        self.env = new_env;
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

    /// Returns true if this `Task` has been exited, i.e.,
    /// if its RunState is either `Exited` or `Reaped`.
    pub fn has_exited(&self) -> bool {
        match self.runstate {
            RunState::Exited(_) | RunState::Reaped => true,
            _ => false,
        }
    }

    /// Returns true if this is an application `Task`.
    pub fn is_application(&self) -> bool {
        self.app_crate.is_some()
    }

    /// Returns true if this is a userspace`Task`.
    pub fn is_userspace(&self) -> bool {
        self.ustack.is_some()
    }

    /// Registers a function or closure that will be called if this `Task` panics.
    pub fn set_panic_handler(&mut self, callback: PanicHandler) {
        self.panic_handler = Some(callback);
    }

    /// Takes ownership of this `Task`'s `PanicHandler` closure/function if one exists,
    /// and returns it so it can be invoked without holding this `Task`'s `Mutex`.
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
    /// # Note
    /// After invoking this, the `Task`'s runstate will be `Reaped`,
    /// and this `Task` will be removed from the system task list.
    pub fn take_exit_value(&mut self) -> Option<ExitValue> {
        match self.runstate {
            RunState::Exited(_) => { }
            _ => return None, 
        }

        let exited = core::mem::replace(&mut self.runstate, RunState::Reaped);
        TASKLIST.lock().remove(&self.id);
        if let RunState::Exited(exit_value) = exited {
            Some(exit_value)
        } 
        else {
            error!("BUG: Task::take_exit_value(): task {} runstate was Exited but couldn't get exit value.", self);
            None
        }
    }

    /// Sets this `Task` as this core's current task.
    /// 
    /// Currently this is achieved by writing a pointer to the `TaskLocalData` 
    /// into the FS segment register base MSR.
    fn set_as_current_task(&self) {
        unsafe {
            wrmsr(IA32_FS_BASE, self.task_local_data_ptr.value() as u64);
        }
    }

    /// Removes this `Task`'s `TaskLocalData` cyclical task reference so that it can be dropped.
    /// This should only be called once, after the Task will never ever be used again. 
    fn take_task_local_data(&mut self) -> Option<Box<TaskLocalData>> {
        // sanity check to ensure we haven't dropped this Task's TaskLocalData twice.
        if self.task_local_data_ptr.value() != 0 {
            let tld = unsafe { Box::from_raw(self.task_local_data_ptr.value() as *mut TaskLocalData) };
            self.task_local_data_ptr = VirtualAddress::zero();
            Some(tld)
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
                error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was pinned to AP {:?} but scheduled on AP {}!\nCurrent: {:?}, Next: {:?}", next.pinned_core, apic_id, self, next);
                my_task_switch_lock.store(false, Ordering::SeqCst);
                return;
            }
        }
         

        // Change the privilege stack (RSP0) in the TSS.
        // We can safely skip setting the TSS RSP0 when switching to a kernel task, 
        // i.e., when `next` is not a userspace task
        if next.is_userspace() {
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

        // update runstates
        self.running_on_cpu = None; // no longer running
        next.running_on_cpu = Some(apic_id); // now running on this core


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
                let next_mmi_locked = next_mmi.lock();
                // debug!("task_switch [3]: switching tables! From {} {:?} to {} {:?}", 
                //         self.name, prev_mmi_locked.page_table, next.name, next_mmi_locked.page_table);

                let _new_active_table = prev_mmi_locked.page_table.switch(&next_mmi_locked.page_table);
            }
        }
       
        // update the current task to `next`
        next.set_as_current_task();

        // If the current task is exited, then we need to remove the cyclical TaskRef reference in its TaskLocalData.
        // We store the removed TaskLocalData in the next Task struct so that we can access it after the context switch.
        if self.has_exited() {
            // trace!("task_switch(): preparing to drop TaskLocalData for running task {}", self);
            next.drop_after_task_switch = self.take_task_local_data().map(|tld_box| tld_box as Box<dyn Any + Send>);
        }

        // release this core's task switch lock
        my_task_switch_lock.store(false, Ordering::SeqCst);
        // debug!("task_switch [4]: prev sp: {:#X}, next sp: {:#X}", self.saved_sp, next.saved_sp);
        

        /// A private macro that actually calls the given context switch routine
        /// by putting the arguments into the proper registers, `rdi` and `rsi`.
        macro_rules! call_context_switch {
            ($func:expr) => ( unsafe {
                asm!("
                    mov rdi, $0; \
                    mov rsi, $1;" 
                    : : "r"(&mut self.saved_sp as *mut usize), "r"(next.saved_sp)
                    : "memory" : "intel", "volatile"
                );
                $func();
            });
        }

        // Now it's time to perform the actual context switch.
        // If `simd_personality` is NOT enabled, then we proceed as normal 
        // using the singular context_switch routine that matches the actual build target. 
        #[cfg(not(simd_personality))]
        {
            call_context_switch!(context_switch::context_switch);
        }
        // If `simd_personality` is enabled, all `context_switch*` routines are available,
        // which allows us to choose one based on whether the prev/next Tasks are SIMD-enabled.
        #[cfg(simd_personality)]
        {
            match (&self.simd, &next.simd) {
                (SimdExt::None, SimdExt::None) => {
                    // warn!("SWITCHING from REGULAR to REGULAR task {:?} -> {:?}", self, next);
                    call_context_switch!(context_switch::context_switch_regular);
                }

                (SimdExt::None, SimdExt::SSE)  => {
                    // warn!("SWITCHING from REGULAR to SSE task {:?} -> {:?}", self, next);
                    call_context_switch!(context_switch::context_switch_regular_to_sse);
                }
                
                (SimdExt::None, SimdExt::AVX)  => {
                    // warn!("SWITCHING from REGULAR to AVX task {:?} -> {:?}", self, next);
                    call_context_switch!(context_switch::context_switch_regular_to_avx);
                }

                (SimdExt::SSE, SimdExt::None)  => {
                    // warn!("SWITCHING from SSE to REGULAR task {:?} -> {:?}", self, next);
                    call_context_switch!(context_switch::context_switch_sse_to_regular);
                }

                (SimdExt::SSE, SimdExt::SSE)   => {
                    // warn!("SWITCHING from SSE to SSE task {:?} -> {:?}", self, next);
                    call_context_switch!(context_switch::context_switch_sse);
                }

                (SimdExt::SSE, SimdExt::AVX) => {
                    warn!("SWITCHING from SSE to AVX task {:?} -> {:?}", self, next);
                    call_context_switch!(context_switch::context_switch_sse_to_avx);
                }

                (SimdExt::AVX, SimdExt::None) => {
                    // warn!("SWITCHING from AVX to REGULAR task {:?} -> {:?}", self, next);
                    call_context_switch!(context_switch::context_switch_avx_to_regular);
                }

                (SimdExt::AVX, SimdExt::SSE) => {
                    warn!("SWITCHING from AVX to SSE task {:?} -> {:?}", self, next);
                    call_context_switch!(context_switch::context_switch_avx_to_sse);
                }

                (SimdExt::AVX, SimdExt::AVX) => {
                    // warn!("SWITCHING from AVX to AVX task {:?} -> {:?}", self, next);
                    call_context_switch!(context_switch::context_switch_avx);
                }
            }
        }
        
        // Here, `self` (curr) is now `next` because the stacks have been switched, 
        // and `next` has become some other random task based on a previous task switch operation.
        // Do not make any assumptions about what `next` is now, since it's unknown. 

        // Now, as a final action, we drop any data that the original previous task 
        // prepared for droppage before the context switch occurred.
        let _prev_task_data_to_drop = self.drop_after_task_switch.take();

    }
}

impl Drop for Task {
    fn drop(&mut self) {
        trace!("Task::drop(): {}", self);
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
/// a reference to a Task is used. 
/// For example, task lists, task spawning, task management, scheduling, etc. 
/// 
/// Essentially a newtype wrapper around `Arc<Lock<Task>>` 
/// where `Lock` is some mutex-like locking type.
/// Currently, `Lock` is a `MutexIrqSafe`, so it **does not** allow
/// multiple readers simultaneously; that will cause deadlock.
/// 
/// `TaskRef` implements the `PartialEq` trait; 
/// two `TaskRef`s are considered equal if they point to the same underlying `Task`.
#[derive(Debug, Clone)]
pub struct TaskRef(
    Arc<(
        MutexIrqSafe<Task>,  // the actual task
        AtomicBool,          // true if it has exited, this is used for faster `join()` calls that avoid disabling interrupts
    )>
); 

// impl Drop for TaskRef {
//     fn drop(&mut self) {
//         trace!("Dropping TaskRef: strong_refs: {}, {:?}", Arc::strong_count(&self.0), self);
//     }
// }

impl TaskRef {
    /// Creates a new `TaskRef` that wraps the given `Task`.
    /// 
    /// Also establishes the `TaskLocalData` struct that will be used 
    /// to determine the current `Task` on each processor core.
    pub fn new(task: Task) -> TaskRef {
        let task_id = task.id;
        let taskref = TaskRef(Arc::new((MutexIrqSafe::new(task), AtomicBool::new(false))));
        let tld = TaskLocalData {
            current_taskref: taskref.clone(),
            current_task_id: task_id,
        };
        let tld_ptr = Box::into_raw(Box::new(tld));
        taskref.0.deref().0.lock().task_local_data_ptr = VirtualAddress::new_canonical(tld_ptr as usize);
        taskref
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
            // if self.0.lock().has_exited() {
            if self.0.deref().1.load(Ordering::SeqCst) == true {
                break;
            }
        }

        // Then, wait for it to actually stop running on any CPU core.
        loop {
            if !self.0.deref().0.lock().is_running() {
                return Ok(());
            }
        }
    }


    /// The internal routine that actually exits or kills a Task.
    /// It also performs select cleanup routines, e.g., removing the task from the task list.
    fn internal_exit(&self, val: ExitValue) -> Result<(), &'static str> {
        {
            let mut task = self.0.deref().0.lock();
            if let RunState::Exited(_) = task.runstate {
                return Err("task was already exited! (did not overwrite its existing exit value)");
            }
            task.runstate = RunState::Exited(val);
            self.0.deref().1.store(true, Ordering::SeqCst);

            // Corner case: if the task isn't running (as with killed tasks), 
            // we must clean it up now rather than in task_switch(), because it will never be scheduled in again. 
            if !task.is_running() {
                trace!("internal_exit(): dropping TaskLocalData for non-running task {}", &*task);
                let _tld = task.take_task_local_data();
            }
        }

        #[cfg(runqueue_state_spill_evaluation)] 
        {   
            let task_on_rq = { self.0.deref().0.lock().on_runqueue.clone() };
            if let Some(remove_from_runqueue) = RUNQUEUE_REMOVAL_FUNCTION.try() {
                if let Some(rq) = task_on_rq {
                    remove_from_runqueue(self, rq)?;
                }
            }
        }

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
    pub fn exit(&self, exit_value: Box<dyn Any + Send>) -> Result<(), &'static str> {
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
    pub fn lock(&self) -> MutexIrqSafeGuardRef<Task> {
        MutexIrqSafeGuardRef::new(self.0.deref().0.lock())
    }

    /// Blocks this `Task` by setting its `RunState` to blocked.
    pub fn block(&self) {
        self.0.deref().0.lock().runstate = RunState::Blocked;
    }

    /// Registers a function or closure that will be called if this `Task` panics.
    /// # Locking / Deadlock
    /// Obtains a write lock on the enclosed `Task` in order to mutate its state.
    pub fn set_panic_handler(&self, callback: PanicHandler) {
        self.0.deref().0.lock().set_panic_handler(callback)
    }

    /// Takes ownership of this `Task`'s `PanicHandler` closure/function if one exists,
    /// and returns it so it can be invoked without holding this `Task`'s `Mutex`.
    /// After invoking this, the `Task`'s `panic_handler` will be `None`.
    /// # Locking / Deadlock
    /// Obtains a write lock on the enclosed `Task` in order to mutate its state.
    pub fn take_panic_handler(&self) -> Option<PanicHandler> {
        self.0.deref().0.lock().take_panic_handler()
    }

    /// Takes ownership of this `Task`'s exit value and returns it,
    /// if and only if this `Task` was in the `Exited` runstate.
    /// After invoking this, the `Task`'s runstate will be `Reaped`.
    /// # Locking / Deadlock
    /// Obtains a write lock on the enclosed `Task` in order to mutate its state.
    pub fn take_exit_value(&self) -> Option<ExitValue> {
        self.0.deref().0.lock().take_exit_value()
    }

    /// Sets the `Environment` of this Task.
    pub fn set_env(&self, new_env: Arc<Mutex<Environment>>) {
        self.0.deref().0.lock().set_env(new_env);
    }

    /// Gets a reference to this task's `Environment`.
    pub fn get_env(&self) -> Arc<Mutex<Environment>> {
        Arc::clone(&self.0.deref().0.lock().env)
    }
    
    /// Obtains the lock on the underlying `Task` in a writeable, blocking fashion.
    #[deprecated(note = "This method exposes inner Task details for debugging purposes. Do not use it.")]
    pub fn lock_mut(&self) -> MutexIrqSafeGuardRefMut<Task> {
        MutexIrqSafeGuardRefMut::new(self.0.deref().0.lock())
    }
}

impl PartialEq for TaskRef {
    fn eq(&self, other: &TaskRef) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for TaskRef { }



/// Create and initialize an idle task, of which there is one per processor core/LocalApic.
/// The idle task is a task that runs by default (one per core) when no other task is running.
/// 
/// Returns a reference to the `Task`, protected by a `RwLockIrqSafe`
/// 
/// # Note
/// This function does not make the new idle task runnable, nor add it to any runqueue.
pub fn create_idle_task(
    apic_id: u8, 
    stack_bottom: VirtualAddress, 
    stack_top: VirtualAddress,
    kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>
) -> Result<TaskRef, &'static str> {

    let mut idle_task = Task::new();
    idle_task.name = format!("idle_task_ap{}", apic_id);
    idle_task.is_an_idle_task = true;
    idle_task.runstate = RunState::Runnable;
    idle_task.running_on_cpu = Some(apic_id); 
    idle_task.pinned_core = Some(apic_id); // can only run on this CPU core
    idle_task.mmi = Some(kernel_mmi_ref);
    // debug!("IDLE TASK STACK (apic {}) at bottom={:#x} - top={:#x} ", apic_id, stack_bottom, stack_top);
    idle_task.kstack = Some( 
        Stack::new( 
            stack_top, 
            stack_bottom, 
            MappedPages::from_existing(
                PageRange::from_virt_addr(stack_bottom, stack_top.value() - stack_bottom.value()),
                EntryFlags::WRITABLE | EntryFlags::PRESENT
            ),
        )
    );

    let idle_task_id = idle_task.id;
    let task_ref = TaskRef::new(idle_task);

    // set this as this core's current task, since it's obviously running
    task_ref.0.deref().0.lock().set_as_current_task();
    if get_my_current_task().is_none() {
        error!("BUG: create_idle_task(): failed to properly set the new idle task as the current task on AP {}", 
            apic_id);
        return Err("BUG: create_idle_task(): failed to properly set the new idle task as the current task");
    }

    // insert the new task into the task list
    let old_task = TASKLIST.lock().insert(idle_task_id, task_ref.clone());
    if old_task.is_some() {
        error!("BUG: create_idle_task(): TASKLIST already contained a task with the same id {} as idle_task_ap{}!", 
            idle_task_id, apic_id);
        return Err("BUG: TASKLIST already contained a task with the new idle_task's ID");
    }
    
    Ok(task_ref)
}



/// The structure that holds information local to each Task,
/// effectively a form of thread-local storage (TLS).
/// A pointer to this structure is stored in the `FS` segment register,
/// such that any task can easily and quickly access their local data.
// #[repr(C)]
#[derive(Debug)]
struct TaskLocalData {
    current_taskref: TaskRef,
    current_task_id: usize,
}

/// Returns a reference to the current task's `TaskLocalData` 
/// by using the `TaskLocalData` pointer stored in the FS base MSR register.
fn get_task_local_data() -> Option<&'static TaskLocalData> {
    let tld: &'static TaskLocalData = {
        let tld_ptr = rdmsr(IA32_FS_BASE) as *const TaskLocalData;
        if tld_ptr.is_null() {
            return None;
        }
        // SAFE: it's safe to cast this as a static reference
        // because it will always be valid for the life of a given Task's execution.
        unsafe { &*tld_ptr }
    };
    Some(&tld)
}

/// Returns a cloned reference to the current task id by using the `TaskLocalData` pointer
/// stored in the FS base MSR register.
pub fn get_my_current_task() -> Option<&'static TaskRef> {
    get_task_local_data().map(|tld| &tld.current_taskref)
}

/// Returns the current Task's id by using the `TaskLocalData` pointer
/// stored in the FS base MSR register.
pub fn get_my_current_task_id() -> Option<usize> {
    get_task_local_data().map(|tld| tld.current_task_id)
}

/// Returns the current Task's working Directory
/// from its `Environment` object.
pub fn get_my_working_dir() -> Option<DirRef> {
    get_my_current_task().map(|taskref| {
        let env = taskref.get_env();
        let env_locked = env.lock();
        Arc::clone(&env_locked.working_dir)
    })
}
