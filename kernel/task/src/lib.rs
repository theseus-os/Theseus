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
#![feature(panic_info_message)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate memory;
extern crate tss;
extern crate mod_mgmt;
extern crate context_switch;
extern crate environment;
extern crate root;
extern crate x86_64;
extern crate spin;
extern crate kernel_config;


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
use irq_safety::{MutexIrqSafe, MutexIrqSafeGuardRef, MutexIrqSafeGuardRefMut, interrupts_enabled};
use memory::{Stack, MappedPages, PageRange, EntryFlags, MmiRef, VirtualAddress};
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use tss::tss_set_rsp0;
use mod_mgmt::{
    CrateNamespace,
    AppCrateRef,
};
use environment::Environment;
use spin::Mutex;
use x86_64::registers::msr::{rdmsr, wrmsr, IA32_FS_BASE};


/// The function signature of the callback that will be invoked
/// when a given Task panics or otherwise fails, e.g., a machine exception occurs.
pub type KillHandler = Box<dyn Fn(&KillReason) + Send>;

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
impl<'p> From<&PanicInfo<'p>> for PanicInfoOwned {
    fn from(info: &PanicInfo) -> PanicInfoOwned {
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
    /// The list of all Tasks in the system.
    pub static ref TASKLIST: MutexIrqSafe<BTreeMap<usize, TaskRef>> = MutexIrqSafe::new(BTreeMap::new());
}


/// returns a shared reference to the `Task` specified by the given `task_id`
pub fn get_task(task_id: usize) -> Option<TaskRef> {
    TASKLIST.lock().get(&task_id).cloned()
}


/// Sets the kill handler function for the current `Task`
pub fn set_my_kill_handler(handler: KillHandler) -> Result<(), &'static str> {
    get_my_current_task()
        .ok_or("couldn't get_my_current_task")
        .map(|taskref| taskref.set_kill_handler(handler))
}



/// The list of possible reasons that a given `Task` was killed prematurely.
#[derive(Debug)]
pub enum KillReason {
    /// The user or another task requested that this `Task` be killed. 
    /// For example, the user pressed `Ctrl + C` on the shell window that started a `Task`.
    Requested,
    /// A Rust-level panic occurred while running this `Task`.
    Panic(PanicInfoOwned),
    /// A non-language-level problem, such as a Page Fault or some other machine exception.
    /// The number of the exception is included, e.g., 15 (0xE) for a Page Fault.
    Exception(u8),
}
impl fmt::Display for KillReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match &self {
            &Self::Requested         => write!(f, "Requested"),
            &Self::Panic(panic_info) => write!(f, "Panicked at {}", panic_info),
            &Self::Exception(num)    => write!(f, "Exception {:#X}({})", num, num),
        }
    }
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


/// The set of possible runstates that a task can be in, e.g.,
/// runnable, blocked, exited, etc. 
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

/// A data structure to hold data related to restart the function. 
/// Presence of `RestartInfo` itself indicates the task will be restartable.
pub struct RestartInfo {
    /// Stores the argument of the task for restartable tasks
    pub argument: Box<dyn Any + Send>,
    /// Stores the function of the task for restartable tasks
    pub func: Box<dyn Any + Send>,
}

/// The signature of a Task's failure cleanup function.
pub type FailureCleanupFunction = fn(TaskRef, KillReason) -> !;


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
    /// Memory management details: page tables, mappings, allocators, etc.
    /// This is shared among all other tasks in the same address space.
    pub mmi: MmiRef, 
    /// The kernel stack, which all `Task`s must have in order to execute.
    pub kstack: Stack,
    /// Whether or not this task is pinned to a certain core.
    /// The idle tasks (like idle_task) are always pinned to their respective cores.
    pub pinned_core: Option<u8>,
    /// Whether this Task is an idle task, the task that runs by default when no other task is running.
    /// There exists one idle task per core, so this is `false` for most tasks.
    pub is_an_idle_task: bool,
    /// For application `Task`s, this is a reference to the [`LoadedCrate`](../mod_mgmt/metadata/struct.LoadedCrate.html)
    /// that contains the entry function for this `Task`.
    pub app_crate: Option<Arc<AppCrateRef>>,
    /// This `Task` is linked into and runs within the context of 
    /// this [`CrateNamespace`](../mod_mgmt/struct.CrateNamespace.html).
    pub namespace: Arc<CrateNamespace>,
    /// The function that will be called when this `Task` panics or fails due to a machine exception.
    /// It will be invoked before the task is cleaned up via stack unwinding.
    /// This is similar to Rust's built-in panic hook, but is also called upon a machine exception, not just a panic.
    pub kill_handler: Option<KillHandler>,
    /// The environment of the task, Wrapped in an Arc & Mutex because it is shared among child and parent tasks
    pub env: Arc<Mutex<Environment>>,
    /// The function that should be run as a last-ditch attempt to recover from this task's failure,
    /// e.g., this can be called when unwinding itself fails. 
    /// Typically, it will point to this Task's specific instance of `spawn::task_cleanup_failure()`,
    /// which has generic type parameters that describe its function signature, argument type, and return type.
    pub failure_cleanup_function: FailureCleanupFunction,
    /// Stores the restartable information of the task. 
    /// `Some(RestartInfo)` indicates that the task is restartable.
    pub restart_info: Option<RestartInfo>,
    
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
    /// By default, the new `Task` will inherit some of the same states from the currently-running `Task`:
    /// its `Environment`, `MemoryManagementInfo`, `CrateNamespace`, and `app_crate` reference.
    /// If needed, those states can be changed by setting them for the returned `Task`.
    /// 
    /// # Arguments
    /// * `kstack`: the optional kernel `Stack` for this `Task` to use.
    ///    If not provided, a kernel stack of the default size will be allocated and used.
    /// 
    /// # Note
    /// This does not run the task, schedule it in, or switch to it.
    /// 
    /// However, it requires tasking to already be set up, i.e., the current task must be known.
    pub fn new(
        kstack: Option<Stack>,
        failure_cleanup_function: FailureCleanupFunction
    ) -> Result<Task, &'static str> {
        let curr_task = get_my_current_task().ok_or("Task::new(): couldn't get current task (not yet initialized)")?;
        let (mmi, namespace, env, app_crate) = {
            let t = curr_task.lock();
            (Arc::clone(&t.mmi), Arc::clone(&t.namespace), Arc::clone(&t.env), t.app_crate.clone())
        };

        let kstack = kstack
            .or_else(|| mmi.lock().alloc_stack(KERNEL_STACK_SIZE_IN_PAGES))
            .ok_or("couldn't allocate kernel stack!")?;

        Ok(Task::new_internal(kstack, mmi, namespace, env, app_crate, failure_cleanup_function))
    }
    
    /// The internal routine for creating a `Task`, which does not make assumptions 
    /// about whether a currently-running `Task` exists or whether the new `Task`
    /// should inherit any states from it.
    fn new_internal(
        kstack: Stack, 
        mmi: MmiRef, namespace: Arc<CrateNamespace>,
        env: Arc<Mutex<Environment>>,
        app_crate: Option<Arc<AppCrateRef>>,
        failure_cleanup_function: FailureCleanupFunction,
    ) -> Self {
         /// The counter of task IDs
        static TASKID_COUNTER: AtomicUsize = AtomicUsize::new(0);

        // we should re-use old task IDs again, instead of simply blindly counting up
        // TODO FIXME: or use random values to avoid state spill
        let task_id = TASKID_COUNTER.fetch_add(1, Ordering::Acquire);

        Task {
            id: task_id,
            runstate: RunState::Initing,
            running_on_cpu: None,
            
            #[cfg(runqueue_state_spill_evaluation)]
            on_runqueue: None,
            
            saved_sp: 0,
            task_local_data_ptr: VirtualAddress::zero(),
            drop_after_task_switch: None,
            name: format!("task_{}", task_id),
            kstack,
            mmi,
            pinned_core: None,
            is_an_idle_task: false,
            app_crate,
            namespace,
            kill_handler: None,
            env,
            failure_cleanup_function,
            restart_info: None,
            
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

    /// Returns `true` if this is an application `Task`. 
    /// This will also return `true` if this task was spawned by an application task,
    /// since a task inherits the "application crate" field from its "parent" who spawned it.
    pub fn is_application(&self) -> bool {
        self.app_crate.is_some()
    }

    /// Returns true if this is a userspace`Task`.
    /// Currently userspace support is disabled, so this always returns `false`.
    pub fn is_userspace(&self) -> bool {
        // self.ustack.is_some()
        false
    }

    /// Registers a function or closure that will be called if this `Task` panics
    /// or otherwise fails (e.g., due to a machine exception occurring).
    /// The given `callback` will be invoked before the task is cleaned up via stack unwinding.
    pub fn set_kill_handler(&mut self, callback: KillHandler) {
        self.kill_handler = Some(callback);
    }

    /// Takes ownership of this `Task`'s `KillHandler` closure/function if one exists,
    /// and returns it so it can be invoked without holding this `Task`'s `Mutex`.
    /// After invoking this, the `Task`'s `kill_handler` will be `None`.
    pub fn take_kill_handler(&mut self) -> Option<KillHandler> {
        self.kill_handler.take()
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
    /// 
    /// # Warning
    /// Before calling this function make sure that the `next` task is runnable, not currently running, and not pinned to another core besides `apic_id`.
    pub fn task_switch(&mut self, next: &mut Task, apic_id: u8) {
        // debug!("task_switch [0]: (AP {}) prev {:?}, next {:?}", apic_id, self, next);         

        // Change the privilege stack (RSP0) in the TSS.
        // We can safely skip setting the TSS RSP0 when switching to a kernel task, 
        // i.e., when `next` is not a userspace task.
        //
        // Note that because userspace support is currently disabled, this will always be `false`.
        if next.is_userspace() {
            let new_tss_rsp0 = next.kstack.bottom() + (next.kstack.size() / 2); // the middle half of the stack
            if tss_set_rsp0(new_tss_rsp0).is_ok() { 
                // debug!("task_switch [2]: new_tss_rsp = {:#X}", new_tss_rsp0);
            }
            else {
                error!("task_switch(): failed to set AP {} TSS RSP0, aborting task switch!", apic_id);
                return;
            }
        }

        // update runstates
        self.running_on_cpu = None; // no longer running
        next.running_on_cpu = Some(apic_id); // now running on this core

        // Switch page tables. 
        // Since there is only a single address space (as userspace support is currently disabled),
        // we do not need to do this at all.
        if false {
            let prev_mmi = &self.mmi;
            let next_mmi = &next.mmi;
            

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

        // debug!("task_switch [4]: prev sp: {:#X}, next sp: {:#X}", self.saved_sp, next.saved_sp);

        /// A private macro that actually calls the given context switch routine.
        macro_rules! call_context_switch {
            ($func:expr) => ( unsafe {
                $func(&mut self.saved_sp as *mut usize, next.saved_sp);
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

        // We must consume/drop the Task's kill handler BEFORE a Task can possibly be dropped.
        // This is because if an application task sets a kill handler that is a closure/function in the text section of the app crate itself,
        // then after the app crate is released, the kill handler will be dropped AFTER the app crate has been freed.
        // When it tries to drop the task's kill handler, a page fault will occur because the text section of the app crate has been unmapped.
        {
            if let Some(_kill_handler) = self.take_kill_handler() {
                warn!("While dropping task {:?}, its kill handler callback was still present. Removing it now.", self);
            }
            // Scoping rules ensure the kill handler is dropped now, before this Task's app_crate could possibly be dropped.
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
    /// This task must be the currently executing task, 
    /// you cannot invoke `mark_as_exited()` on a different task.
    /// 
    /// This should only be used at the end of the `task_wrapper` function once it has cleanly exited.
    /// 
    /// # Locking / Deadlock
    /// This method obtains a writable lock on the underlying Task in order to mutate its state.
    /// 
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set.     
    /// * Returns `Err` if this `Task` was already exited, and does not overwrite the existing exit status. 
    ///  
    /// # Note 
    /// The `Task` will not be halted immediately -- 
    /// it will finish running its current timeslice, and then never be run again.
    #[doc(hidden)]
    pub fn mark_as_exited(&self, exit_value: Box<dyn Any + Send>) -> Result<(), &'static str> {
        let curr_task = get_my_current_task().ok_or("mark_as_exited(): failed to check what the current task is")?;
        if curr_task == self {
            self.internal_exit(ExitValue::Completed(exit_value))
        } else {
            Err("`mark_as_exited()` can only be invoked on the current task, not on another task.")
        }
    }

    /// Call this function to indicate that this task has been cleaned up (e.g., by unwinding)
    /// and it is ready to be marked as killed, i.e., it will never run again.
    /// This task must be the currently executing task, 
    /// you cannot invoke `mark_as_killed()` on a different task.
    /// 
    /// If you want to kill another task, use the [`kill()`](method.kill) method instead.
    /// 
    /// This should only be used by the unwinding routines once they have finished.
    /// 
    /// # Locking / Deadlock
    /// This method obtains a writable lock on the underlying Task in order to mutate its state.
    /// 
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set.     
    /// * Returns `Err` if this `Task` was already exited, and does not overwrite the existing exit status. 
    ///  
    /// # Note 
    /// The `Task` will not be halted immediately -- 
    /// it will finish running its current timeslice, and then never be run again.
    #[doc(hidden)]
    pub fn mark_as_killed(&self, reason: KillReason) -> Result<(), &'static str> {
        let curr_task = get_my_current_task().ok_or("mark_as_exited(): failed to check what the current task is")?;
        if curr_task == self {
            self.internal_exit(ExitValue::Killed(reason))
        } else {
            Err("`mark_as_exited()` can only be invoked on the current task, not on another task.")
        }
    }

    /// Kills this `Task` (not a clean exit) without allowing it to run to completion.
    /// The provided `KillReason` indicates why it was killed.
    /// 
    /// **
    /// Currently this immediately kills the task without performing any unwinding cleanup.
    /// In the near future, the task will be unwound such that its resources are freed/dropped
    /// to ensure proper cleanup before the task is actually fully killed.
    /// **
    /// 
    /// # Locking / Deadlock
    /// This method obtains a writable lock on the underlying Task in order to mutate its state.
    /// 
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set to the given `KillReason`.     
    /// * Returns `Err` if this `Task` was already exited, and does not overwrite the existing exit status. 
    /// 
    /// # Note 
    /// The `Task` will not be halted immediately -- 
    /// it will finish running its current timeslice, and then never be run again.
    pub fn kill(&self, reason: KillReason) -> Result<(), &'static str> {
        // TODO FIXME: cause a panic in this Task such that it will start the unwinding process
        // instead of immediately causing it to exit
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

    /// Unblocks this `Task` by setting its `RunState` to runnable.
    pub fn unblock(&self) {
        self.0.deref().0.lock().runstate = RunState::Runnable;
    }

    /// Registers a function or closure that will be called if this `Task` panics
    /// or otherwise fails (e.g., due to a machine exception). 
    /// The given `callback` will be invoked before the task is cleaned up via stack unwinding.
    /// # Locking / Deadlock
    /// Obtains a write lock on the enclosed `Task` in order to mutate its state.
    pub fn set_kill_handler(&self, callback: KillHandler) {
        self.0.deref().0.lock().set_kill_handler(callback)
    }

    /// Takes ownership of this `Task`'s `KillHandler` closure/function if one exists,
    /// and returns it so it can be invoked without holding this `Task`'s `Mutex`.
    /// After invoking this, the `Task`'s `kill_handler` will be `None`.
    /// # Locking / Deadlock
    /// Obtains a write lock on the enclosed `Task` in order to mutate its state.
    pub fn take_kill_handler(&self) -> Option<KillHandler> {
        self.0.deref().0.lock().take_kill_handler()
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

    /// Gets a reference to this task's `CrateNamespace`.
    pub fn get_namespace(&self) -> Arc<CrateNamespace> {
        Arc::clone(&self.0.deref().0.lock().namespace)
    }
    
    /// Obtains the lock on the underlying `Task` in a writable, blocking fashion.
    #[deprecated(note = "This method exposes inner Task details for debugging purposes. Do not use it.")]
    #[doc(hidden)]
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



/// Bootstrap a new task from the current thread of execution.
/// 
/// # Note
/// This function does not add the new task to any runqueue.
pub fn bootstrap_task(
    apic_id: u8, 
    stack_bottom: VirtualAddress, 
    stack_top: VirtualAddress,
    kernel_mmi_ref: MmiRef,
) -> Result<TaskRef, &'static str> {
    // Here, we cannot call `Task::new()` because tasking hasn't yet been set up for this core.
    // Instead, we generate all of the `Task` states manually, and create an initial task directly.
    let kstack = Stack::new( 
        stack_top, 
        stack_bottom, 
        MappedPages::from_existing(
            PageRange::from_virt_addr(stack_bottom, stack_top.value() - stack_bottom.value()),
            EntryFlags::WRITABLE | EntryFlags::PRESENT
        ),
    );
    let default_namespace = mod_mgmt::get_initial_kernel_namespace()
        .ok_or("The initial kernel CrateNamespace must be initialized before the tasking subsystem.")?
        .clone();
    let default_env = Arc::new(Mutex::new(Environment::default()));
    let mut bootstrap_task = Task::new_internal(kstack, kernel_mmi_ref, default_namespace, default_env, None, bootstrap_task_cleanup_failure);
    bootstrap_task.name = format!("bootstrap_task_core_{}", apic_id);
    bootstrap_task.runstate = RunState::Runnable;
    bootstrap_task.running_on_cpu = Some(apic_id); 
    bootstrap_task.pinned_core = Some(apic_id); // can only run on this CPU core
    // debug!("IDLE TASK STACK (apic {}) at bottom={:#x} - top={:#x} ", apic_id, stack_bottom, stack_top);
    let bootstrap_task_id = bootstrap_task.id;
    let task_ref = TaskRef::new(bootstrap_task);

    // set this as this core's current task, since it's obviously running
    task_ref.0.deref().0.lock().set_as_current_task();
    if get_my_current_task().is_none() {
        error!("BUG: bootstrap_task(): failed to properly set the new idle task as the current task on AP {}", apic_id);
        return Err("BUG: bootstrap_task(): failed to properly set the new idle task as the current task");
    }

    // insert the new task into the task list
    let old_task = TASKLIST.lock().insert(bootstrap_task_id, task_ref.clone());
    if let Some(ot) = old_task {
        error!("BUG: bootstrap_task(): TASKLIST already contained a task {:?} with the same id {} as bootstrap_task_core_{}!", 
            ot, bootstrap_task_id, apic_id
        );
        return Err("BUG: bootstrap_task(): TASKLIST already contained a task with the new bootstrap_task's ID");
    }
    
    Ok(task_ref)
}


/// This is just like `spawn::task_cleanup_failure()`,
/// but for the initial tasks bootstrapped from each core's first execution context.
/// 
/// However, for a bootstrapped task, we don't know its function signature, argument type, or return value type
/// because it was invoked from assembly and may not even have one. 
/// 
/// Therefore there's not much we can actually do.
fn bootstrap_task_cleanup_failure(current_task: TaskRef, kill_reason: KillReason) -> ! {
    loop {
        error!("BUG: bootstrap_task_cleanup_failure: {:?} died with {:?}\n. There's nothing we can do here; looping indefinitely!", current_task.lock().name, kill_reason);
    }
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
    Some(tld)
}

/// Returns a reference to the current task by using the `TaskLocalData` pointer
/// stored in the thread-local storage (FS base model-specific register).
pub fn get_my_current_task() -> Option<&'static TaskRef> {
    get_task_local_data().map(|tld| &tld.current_taskref)
}

/// Returns the current Task's id by using the `TaskLocalData` pointer
/// stored in the thread-local storage (FS base model-specific register).
pub fn get_my_current_task_id() -> Option<usize> {
    get_task_local_data().map(|tld| tld.current_task_id)
}
