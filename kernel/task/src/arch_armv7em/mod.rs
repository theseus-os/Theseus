use core::fmt;
use core::any::Any;
use core::panic::PanicInfo;
use core::sync::atomic::{Ordering, AtomicUsize, AtomicBool};
use alloc::string::String;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::collections::BTreeMap;
use memory::{VirtualAddress, get_frame_allocator_ref};
use stack::Stack;
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use spin::Mutex;
use irq_safety::{MutexIrqSafe, MutexIrqSafeGuardRef, MutexIrqSafeGuardRefMut, interrupts_enabled};
use core::ops::Deref;
use environment::Environment;


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

/// Task local data pointer.
static mut TLDPTR: usize = 0;

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
    /// the runnability status of this task, basically whether it's allowed to be scheduled in.
    pub runstate: RunState,
    /// the saved stack pointer value, used for task switching.
    pub saved_sp: usize,
    /// the virtual address of (a pointer to) the `TaskLocalData` struct, which refers back to this `Task` struct.
    task_local_data_ptr: VirtualAddress,
    /// Data that should be dropped after a task switch; for example, the previous Task's TaskLocalData.
    drop_after_task_switch: Option<Box<dyn Any + Send>>,
    /// The kernel stack, which all `Task`s must have in order to execute.
    pub kstack: Stack,
    /// Whether or not this task is pinned to a certain core.
    /// The idle tasks (like idle_task) are always pinned to their respective cores.
    pub pinned_core: Option<u8>,
    /// Whether this Task is an idle task, the task that runs by default when no other task is running.
    /// There exists one idle task per core, so this is `false` for most tasks.
    pub is_an_idle_task: bool,
    /// The environment of the task, Wrapped in an Arc & Mutex because it is shared among child and parent tasks
    pub env: Arc<Mutex<Environment>>,
    /// The function that should be run as a last-ditch attempt to recover from this task's failure,
    /// e.g., this can be called when unwinding itself fails. 
    /// Typically, it will point to this Task's specific instance of `spawn::task_cleanup_failure()`,
    /// which has generic type parameters that describe its function signature, argument type, and return type.
    pub failure_cleanup_function: FailureCleanupFunction,
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
        let env = {
            let t = curr_task.lock();
            Arc::clone(&t.env)
        };

        let kstack = kstack
            .or_else(|| get_frame_allocator_ref().and_then(|_| 
                stack::alloc_stack(KERNEL_STACK_SIZE_IN_PAGES)
            ))
            .ok_or("couldn't allocate kernel stack!")?;

        Ok(Task::new_internal(kstack, env, failure_cleanup_function))
    }
    
    /// The internal routine for creating a `Task`, which does not make assumptions 
    /// about whether a currently-running `Task` exists or whether the new `Task`
    /// should inherit any states from it.
    fn new_internal(
        kstack: Stack, 
        env: Arc<Mutex<Environment>>,
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

            #[cfg(runqueue_spillful)]
            on_runqueue: None,

            saved_sp: 0,
            task_local_data_ptr: VirtualAddress::zero(),
            drop_after_task_switch: None,
            name: format!("task_{}", task_id),
            kstack,
            pinned_core: None,
            is_an_idle_task: false,
            env,
            failure_cleanup_function
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

    /// Returns true if this is a userspace`Task`.
    /// Currently userspace support is disabled, so this always returns `false`.
    pub fn is_userspace(&self) -> bool {
        // self.ustack.is_some()
        false
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
    /// into the global static variable.
    fn set_as_current_task(&self) {
        unsafe {
            TLDPTR = self.task_local_data_ptr.value() as usize;
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

    /// Switches from the current (`self`)  to the given `next` Task.
    /// 
    /// No locks need to be held to call this, but interrupts (later, preemption) should be disabled.
    pub fn task_switch(&mut self, next: &mut Task, apic_id: u8) {
        // debug!("task_switch [0]: (AP {}) prev {:?}, next {:?}", apic_id, self, next);         

        // These conditions are checked elsewhere, but can be re-enabled if we want to be extra strict.
        // if !next.is_runnable() {
        //     error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was not Runnable! Current: {:?}, Next: {:?}", self, next);
        //     return;
        // }
        // if next.is_running() {
        //     error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was already running on AP {}!\nCurrent: {:?} Next: {:?}", apic_id, self, next);
        //     return;
        // }
        // if let Some(pc) = next.pinned_core {
        //     if pc != apic_id {
        //         error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was pinned to AP {:?} but scheduled on AP {}!\nCurrent: {:?}, Next: {:?}", next.pinned_core, apic_id, self, next);
        //         return;
        //     }
        // }

        // Note that because userspace support is currently disabled, this will never happen.
        // // Change the privilege stack (RSP0) in the TSS.
        // // We can safely skip setting the TSS RSP0 when switching to a kernel task, 
        // // i.e., when `next` is not a userspace task.
        // //
        // if next.is_userspace() {
        //     let new_tss_rsp0 = next.kstack.bottom() + (next.kstack.size() / 2); // the middle half of the stack
        //     if tss_set_rsp0(new_tss_rsp0).is_ok() { 
        //         // debug!("task_switch [2]: new_tss_rsp = {:#X}", new_tss_rsp0);
        //     }
        //     else {
        //         error!("task_switch(): failed to set AP {} TSS RSP0, aborting task switch!", apic_id);
        //         return;
        //     }
        // }

        // update runstates
        self.running_on_cpu = None; // no longer running
        next.running_on_cpu = Some(apic_id); // now running on this core
       
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
        #[cfg(not(any(rq_eval, downtime_eval)))]
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
                // trace!("internal_exit(): dropping TaskLocalData for non-running task {}", &*task);
                let _tld = task.take_task_local_data();
            }
        }

        #[cfg(runqueue_spillful)] 
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
    /// This should only be used within task cleanup functions to indicate
    /// that the current task has cleanly exited.
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
        self.internal_exit(ExitValue::Completed(exit_value))
    }

    /// Call this function to indicate that this task has been cleaned up (e.g., by unwinding)
    /// and it is ready to be marked as killed, i.e., it will never run again.
    /// This task (`self`) must be the currently executing task, 
    /// you cannot invoke `mark_as_killed()` on a different task.
    /// 
    /// If you want to kill another task, use the [`kill()`](method.kill) method instead.
    /// 
    /// This should only be used within task cleanup functions (e.g., after unwinding) to indicate
    /// that the current task has crashed or failed and has been killed by the system.
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
    
    /// Obtains the lock on the underlying `Task` in a writable, blocking fashion.
    #[deprecated(note = "This method exposes inner Task details for debugging purposes. Do not use it.")]
    #[doc(hidden)]
    pub fn lock_mut(&self) -> MutexIrqSafeGuardRefMut<Task> {
        MutexIrqSafeGuardRefMut::new(self.0.deref().0.lock())
    }

    pub fn is_restartable(&self) -> bool {
        false
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
    stack: Stack,
) -> Result<TaskRef, &'static str> {
    // Here, we cannot call `Task::new()` because tasking hasn't yet been set up for this core.
    // Instead, we generate all of the `Task` states manually, and create an initial task directly.
    let default_env = Arc::new(Mutex::new(Environment::default()));
    let mut bootstrap_task = Task::new_internal(stack, default_env, bootstrap_task_cleanup_failure);
    bootstrap_task.name = format!("bootstrap_task_core_{}", apic_id);
    bootstrap_task.runstate = RunState::Runnable;
    bootstrap_task.running_on_cpu = Some(apic_id); 
    bootstrap_task.pinned_core = Some(apic_id); // can only run on this CPU core
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
    error!("BUG: bootstrap_task_cleanup_failure: {:?} died with {:?}\n. There's nothing we can do here; looping indefinitely!", current_task.lock().name, kill_reason);
    loop { }
}


/// The structure that holds information local to each Task,
/// effectively a form of thread-local storage (TLS).
/// A pointer to this structure is stored in the `FS` segment register,
/// such that any task can easily and quickly access their local data.
#[derive(Debug)]
struct TaskLocalData {
    current_taskref: TaskRef,
    current_task_id: usize,
}

/// Returns a reference to the current task's `TaskLocalData` 
/// by using the `TaskLocalData` pointer stored in the global static variable.
fn get_task_local_data() -> Option<&'static TaskLocalData> {
    let tld: &'static TaskLocalData = {
        let tld_ptr = unsafe { TLDPTR } as *const TaskLocalData;
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
