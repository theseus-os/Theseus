//! Key types and functions for multitasking that build on the basic [`Task`].
//!
//! The main types of interest are:
//! 1. [`TaskRef`]: a shareable reference to a `Task` that can actually be used,
//!    unlike the basic `Task` type that cannot be spawned, modified, or scheduled in.
//! 2. [`JoinableTaskRef`]: a derivative of `TaskRef` that allows the owner
//!    (a different task) to *join* that task, i.e., wait for it to exit,
//!    and to retrieve its [`ExitValue`].
//!
//! The main standalone functions allow one to:
//! 1. Obtain the current task:
//!    * [`with_current_task()`] is the preferred way, which accepts a closure
//!      that is invoked with access to the current task. This is preferred because
//!      it doesn't need to clone the current task reference and is thus most efficient.
//!    * [`get_my_current_task()`] returns a cloned reference to the current task
//!      and is thus slightly more expensive [`with_current_task()`].
//!    * [`get_my_current_task_id()`] is fastest if you just want the ID of the current task.
//!      Note that it is fairly expensive to obtain a task reference from a task ID.
//! 2. Register a kill handler for the current task -- [`set_kill_handler()`].
//! 3. Yield the current CPU and schedule in another task -- [`schedule()`].
//! 4. Switch from the current task to another specific "next" task -- [`task_switch()`].
//!
//! To create new task, use the task builder functions in [`spawn`](../spawn/index.html)
//! rather than attempting to manually instantiate a `TaskRef`.

#![no_std]
#![feature(negative_impls)]
#![feature(thread_local)]

extern crate alloc;

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    format,
    sync::{Arc, Weak}, vec::Vec,
};
use core::{
    any::Any,
    cell::RefMut,
    fmt,
    hash::{Hash, Hasher},
    ops::Deref,
    sync::atomic::{AtomicBool, fence, Ordering},
    task::Waker,
};
use cpu::CpuId;
use crossbeam_utils::atomic::AtomicCell;
use irq_safety::hold_interrupts;
use log::error;
use environment::Environment;
use memory::MmiRef;
use no_drop::NoDrop;
use preemption::PreemptionGuard;
use spin::Mutex;
use sync_irq::IrqSafeMutex;
use stack::Stack;
use task_struct::ExposedTask;


// Re-export main types from `task_struct`.
pub use task_struct::{
    ExitValue, InheritedStates, KillHandler, KillReason,
    PanicInfoOwned, RestartInfo, RunState, Task,
};
#[cfg(simd_personality)]
pub use task_struct::SimdExt;


/// The list of all Tasks in the system.
static TASKLIST: IrqSafeMutex<BTreeMap<usize, TaskRef>> = IrqSafeMutex::new(BTreeMap::new());

/// Returns a `WeakTaskRef` (shared reference) to the `Task` specified by the given `task_id`.
pub fn get_task(task_id: usize) -> Option<WeakTaskRef> {
    TASKLIST.lock().get(&task_id).map(TaskRef::downgrade)
}

/// Returns a list containing a snapshot of all tasks that currently exist.
///
/// # Usage Notes
/// * This is an expensive and slow function, so it should be used rarely.
/// * The existence of a task in the returned list does not mean the task will continue to exist
///   at any point in the future, hence the return type of `WeakTaskRef` instead of `TaskRef`.
pub fn all_tasks() -> Vec<(usize, WeakTaskRef)> {
    let tasklist = TASKLIST.lock();
    let mut v = Vec::with_capacity(tasklist.len());
    v.extend(tasklist.iter().map(|(id, t)| (*id, t.downgrade())));
    v
}


/// The signature of a Task's failure cleanup function.
pub type FailureCleanupFunction = fn(ExitableTaskRef, KillReason) -> !;


/// A shareable, cloneable reference to a `Task` that exposes more methods
/// for task management and auto-derefs into an immutable `&Task` reference.
///
/// The `TaskRef` type is necessary because in many places across Theseus,
/// a reference to a Task is used. 
/// For example, task lists, task spawning, task management, scheduling, etc. 
///
/// ## Equality comparisons
/// `TaskRef` implements the [`PartialEq`] and [`Eq`] traits to ensure that
/// two `TaskRef`s are considered equal if they point to the same underlying `Task`.
#[derive(Clone)]
pub struct TaskRef(Arc<TaskRefInner>);
impl fmt::Debug for TaskRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskRef")
            .field("task", &self.0.task.task)
            .finish_non_exhaustive()
    }
}
struct TaskRefInner {
    /// A wrapper around `Task` that allows us to access all of its internal state.
    /// Note that the deref implementation of `TaskRef` bypasses this as to not expose
    /// all internal states of a `Task`, rather only the ones made public in `Task` itself.
    task: ExposedTask,
    /// The function that should be run as a last-ditch attempt to recover from this task's failure,
    /// e.g., this can be called when unwinding itself fails. 
    /// Typically, it will point to this Task's specific instance of `spawn::task_cleanup_failure()`,
    /// which has generic type parameters that describe its function signature, argument type, and return type.
    failure_cleanup_function: FailureCleanupFunction,
    /// A mailbox for exchanging a task's exit value with another task waiting to join on it.
    exit_value_mailbox: Mutex<Option<ExitValue>>,
    /// Whether this Task is joinable.
    /// * If `true`, another task holds the `JoinableTaskRef` object that was created
    ///   by `TaskRef::create()`, which indicates that that other task is able to
    ///   wait for this task to exit and thus be able to obtain this task's exit value.
    /// * If `false`, the `JoinableTaskRef` was dropped, and therefore no other task
    ///   can join this task or obtain its exit value.
    ///
    /// This is not public because it permits interior mutability.
    joinable: AtomicBool,
}

impl TaskRef {
    /// Creates a new `TaskRef`, a shareable wrapper around the given `Task`.
    ///
    /// This does *not* add this task to the system-wide task list or any runqueues,
    /// nor does it schedule this task in.
    ///
    /// # Arguments
    /// * `task`: the new `Task` to wrap in a `TaskRef`.
    /// * `failure_cleanup_function`: an error handling function that acts as a last resort
    ///    when all else fails, e.g., if unwinding fails.
    ///
    /// # Return
    /// Returns a [`JoinableTaskRef`], which derefs into the newly-created `TaskRef`
    /// and can be used to "join" this task (wait for it to exit) and obtain its exit value.
    pub fn create(
        task: Task,
        failure_cleanup_function: FailureCleanupFunction,
    ) -> JoinableTaskRef {
        let exit_value_mailbox = Mutex::new(None);
        let taskref = TaskRef(Arc::new(TaskRefInner {
            task: task.into(),
            failure_cleanup_function,
            exit_value_mailbox,
            // A new task is joinable until its `JoinableTaskRef` is dropped.
            joinable: AtomicBool::new(true),
        }));

        // Add the new TaskRef to the global task list.
        let _existing_task = TASKLIST.lock().insert(taskref.id, taskref.clone());
        assert!(_existing_task.is_none(), "BUG: TASKLIST contained a task with the same ID");

        JoinableTaskRef { task: taskref }
    }

    /// Creates a new weak reference to this `Task`, similar to [`Weak`].
    pub fn downgrade(&self) -> WeakTaskRef {
        WeakTaskRef(Arc::downgrade(&self.0))
    }

    /// Returns `true` if this task is joinable, `false` if not.
    /// 
    /// * If `true`, another task holds the [`JoinableTaskRef`] object that was created
    ///   by [`TaskRef::create()`], which indicates that that other task is able to
    ///   wait for this task to exit and thus be able to obtain this task's exit value.
    /// * If `false`, the [`JoinableTaskRef`] object was dropped, and therefore no other task
    ///   can join this task or obtain its exit value.
    /// 
    /// When a task is not joinable, it is considered to be an orphan
    /// and will thus be automatically reaped and cleaned up once it exits
    /// because no other task is waiting on it to exit.
    #[doc(alias("orphan", "zombie"))]
    pub fn is_joinable(&self) -> bool {
        self.0.joinable.load(Ordering::Relaxed)
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
    /// This method obtains a writable lock on the underlying Task's inner state.
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

    /// The internal routine that actually exits or kills a Task.
    fn internal_exit(&self, val: ExitValue) -> Result<(), &'static str> {
        if self.has_exited() {
            return Err("BUG: task was already exited! (did not overwrite its existing exit value)");
        }
        {
            *self.0.exit_value_mailbox.lock() = Some(val);
            self.0.task.runstate().store(RunState::Exited);

            // Synchronize with the acquire fence in `JoinableTaskRef::join()`,
            // as we have just stored the exit value that `join()` will load.
            fence(Ordering::Release);

            // Now that we have set the exit value and marked the task as exited,
            // it is safe to wake any other tasks that are waiting for this task to exit.
            if let Some(waker) = self.0.task.inner().lock().waker.take() {
                waker.wake();
            }

            // Corner case: if the task isn't currently running (as with killed tasks), 
            // we must clean it up now rather than in `task_switch()`, as it will never be scheduled in again.
            if !self.is_running() {
                todo!("Unhandled scenario: internal_exit(): task {:?} wasn't running \
                    but its current task TLS variable needs to be cleaned up!", &self.0.task.task);
                // Note: we cannot call `deinit_current_task()` here because if this task
                //       isn't running, then it's definitely not the current task.
                //
                // let _taskref_in_tls = deinit_current_task();
                // drop(_taskref_in_tls);
            }
        }
        Ok(())
    }

    /// Takes the `ExitValue` from this `Task` and returns it
    /// if and only if this `Task` was in the `Exited` runstate.
    ///
    /// If this `Task` was in the `Exited` runstate, after invoking this,
    /// this `Task`'s runstate will be set to `Reaped`
    /// and this `Task` will be removed from the system task list.
    ///
    /// If this `Task` was **not** in the `Exited` runstate, 
    /// nothing is done and `None` is returned.
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on the system task list.
    fn reap_exit_value(&self) -> Option<ExitValue> {
        if self.0.task.runstate().compare_exchange(RunState::Exited, RunState::Reaped).is_ok() {
            TASKLIST.lock().remove(&self.id);
            self.0.exit_value_mailbox.lock().take()
        } else {
            None
        }
    }

    /// Sets this `Task` as this CPU's current task.
    ///
    /// Currently, this simply updates the current CPU's TLS base register
    /// to point to this task's TLS data image.
    fn set_as_current_task(&self) {
        // SAFETY: We don't drop the TLS area until the task is finished.
        unsafe { self.0.task.tls_area().set_as_current_tls() };
    }
}

impl PartialEq for TaskRef {
    fn eq(&self, other: &TaskRef) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for TaskRef { }

impl Hash for TaskRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl Deref for TaskRef {
    type Target = Task;
    fn deref(&self) -> &Self::Target {
        &self.0.task.task
    }
}

// ---- The below Drop handler is only used for debugging ----
// impl Drop for TaskRef {
//     fn drop(&mut self) {
//         trace!("[Curr {}] Dropping TaskRef: strong_count: {}, {:?}",
//             get_my_current_task_id(),
//             Arc::strong_count(&self.0),
//             self,
//         );
//     }
// }


/// A weak reference to a shared Task reference (`TaskRef`).
///
/// `WeakTaskRef` and `TaskRef` behave analogously to [`Weak`] and [`Arc`];
/// see the documentation of [`Weak`] for more detail.
///
/// This is created via [`TaskRef::downgrade()`].
#[derive(Clone)]
pub struct WeakTaskRef(Weak<TaskRefInner>);
impl WeakTaskRef {
    /// Attempts to upgrade this `WeakTaskRef` to a `TaskRef`; see [`Weak::upgrade()`].
    ///
    /// Returns `None` if the `TaskRef` has already been dropped, meaning that the
    /// `Task` itself no longer exists has been exited, cleaned up, and fully dropped.
    pub fn upgrade(&self) -> Option<TaskRef> {
        self.0.upgrade().map(TaskRef)
    }
}
impl fmt::Debug for WeakTaskRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(WeakTaskRef)")
    }
}


/// A reference to a `Task` that can be `join`ed; auto-derefs into [`TaskRef`].
///
/// This allows another task to [`join`] this task, i.e., wait for this task
/// to finish executing, and to obtain its [`ExitValue`] thereafter.
///
/// ## [`Drop`]-based Behavior
/// The contained [`Task`] is joinable until this object is dropped.
/// When dropped, this task will be marked as non-joinable and treated as an "orphan" task.
/// This means that there is no way for another task to wait for it to complete
/// or obtain its exit value.
/// As such, this task will be auto-reaped after it exits (in order to avoid zombie tasks).
///
/// ## Not `Clone`-able
/// Due to the above drop-based behavior, this type does not implement `Clone`
/// because it assumes there is only ever one `JoinableTaskRef` per task.
///
/// However, this type auto-derefs into an inner [`TaskRef`],
/// which *can* be cloned, so you can easily call `.clone()` on it.
///
/// [`join`]: [JoinableTaskRef::join]
//
// /// Note: this type is considered an internal implementation detail.
// /// Instead, use the `TaskJoiner` type from the `spawn` crate, 
// /// which is intended to be the public-facing interface for joining a task.
pub struct JoinableTaskRef {
    task: TaskRef,
}
static_assertions::assert_not_impl_any!(JoinableTaskRef: Clone);
impl fmt::Debug for JoinableTaskRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinableTaskRef")
            .field("task", &self.task)
            .finish_non_exhaustive()
    }
}
impl Deref for JoinableTaskRef {
    type Target = TaskRef;
    fn deref(&self) -> &Self::Target {
        &self.task
    }
}
impl JoinableTaskRef {
    /// Sets the waker to be awoken when this task exits.
    pub fn set_waker(&self, waker: Waker) {
        self.task.0.task.inner().lock().waker = Some(waker);
    }

    /// Blocks the current task until this task has exited.
    ///
    /// Synchronizes memory with respect to the joined task.
    ///
    /// # Return
    /// * `Ok` containing this `Task`'s [`ExitValue`] once this task has exited.
    ///   * This includes cases where this `Task` failed or was killed.
    /// * `Err` if there was a problem while waiting for this task to exit.
    ///   * This does *not* include cases where this `Task` failed or was killed,
    ///     rather only cases where the `join` operation itself failed.
    #[doc(alias("reap", "exit"))]
    pub fn join(&self) -> Result<ExitValue, &'static str> {        
        if !self.has_exited() {
            // Create a waker+blocker pair that will block the current task
            // and then wake it once this task (`self`) exits.
            let curr_task = get_my_current_task().ok_or("join(): couldn't get current task")?;
            let task_to_block = curr_task.clone();
            let wake_action = move || {
                let _ = curr_task.unblock();
            };
            let (waker, blocker) = waker_generic::new_waker(wake_action);
            self.set_waker(waker);
            let block_action = || {
                let _ = task_to_block.block();
                ScheduleOnDrop { }
            };
            blocker.block(block_action);
        }
        
        // Note: previously, we waited for this task to actually stop running,
        //       but this isn't actually necessary since we only care whether
        //       the task has exited and its exit value has been written.
        // while self.is_running() { }

        // Synchronize with the release fence in`TaskRef::internal_exit`
        // when the exit value for this task was stored.
        fence(Ordering::Acquire);

        self.reap_exit_value()
            .ok_or("BUG: `join()` could not retrieve `ExitValue` after task had exited.")
    }
}
impl Drop for JoinableTaskRef {
    /// Marks the inner [`Task`] as not joinable, meaning that it is an orphaned task
    /// that will be auto-reaped after exiting.
    fn drop(&mut self) {
        self.0.joinable.store(false, Ordering::Relaxed);
    }
}

/// An empty struct that invokes [`schedule()`] when it is dropped.
pub struct ScheduleOnDrop { }
impl Drop for ScheduleOnDrop {
    fn drop(&mut self) {
        schedule();
    }
}


/// A wrapper around `TaskRef` that allows this task to mark itself as exited.
///
/// This is primarily an internal implementation details, as it is only obtainable
/// when a task is first switched to, specifically while it is executing the
/// `spawn::task_wrapper()` (before it proceeds to running its actual entry function).
///
/// ## Not `Clone`-able
/// This type does not implement `Clone` because it assumes there is
/// only ever one `ExitableTaskRef` per task.
///
/// However, this type auto-derefs into an inner [`TaskRef`],
/// which *can* be cloned, so you can easily call `.clone()` on it.
pub struct ExitableTaskRef {
    task: TaskRef,
}
// Ensure that `ExitableTaskRef` cannot be moved to (Send) or shared with (Sync)
// another task, as a task is the only one who should be able to mark itself as exited.
impl !Send for ExitableTaskRef { }
impl !Sync for ExitableTaskRef { }
impl fmt::Debug for ExitableTaskRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExitableTaskRef")
            .field("task", &self.task)
            .finish_non_exhaustive()
    }
}
impl Deref for ExitableTaskRef {
    type Target = TaskRef;
    fn deref(&self) -> &Self::Target {
        &self.task
    }
}
impl ExitableTaskRef {
    /// Call this function to indicate that this task has successfully ran to completion,
    /// and that it has returned the given `exit_value`.
    /// 
    /// This is only usable within task cleanup functions to indicate
    /// that the current task has cleanly exited.
    /// 
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set.     
    /// * Returns `Err` if this `Task` was already exited;
    ///   the existing exit status will not be overwritten.
    ///  
    /// # Note 
    /// The `Task` will not be halted immediately -- 
    /// it will finish running its current timeslice, and then never be run again.
    pub fn mark_as_exited(&self, exit_value: Box<dyn Any + Send>) -> Result<(), &'static str> {
        self.internal_exit(ExitValue::Completed(exit_value))
    }

    /// Call this function to indicate that this task has been cleaned up (e.g., by unwinding)
    /// and it is ready to be marked as killed, i.e., it will never run again.
    /// 
    /// If you want to kill another task, use [`TaskRef::kill()`] instead.
    /// 
    /// This is only usable within task cleanup functions (e.g., after unwinding) to indicate
    /// that the current task has crashed or failed and has been killed by the system.
    /// 
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set.     
    /// * Returns `Err` if this `Task` was already exited, and does not overwrite the existing exit status. 
    ///  
    /// # Note 
    /// The `Task` will not be halted immediately -- 
    /// it will finish running its current timeslice, and then never be run again.
    pub fn mark_as_killed(&self, reason: KillReason) -> Result<(), &'static str> {
        self.internal_exit(ExitValue::Killed(reason))
    }

    /// Reaps this task (if orphaned) by taking and dropping its exit value and removing it
    /// from the system task list.
    ///
    /// If this task has *not* been orphaned, meaning it is still joinable,
    /// then this function does nothing.
    pub fn reap_if_orphaned(&self) {
        if !self.is_joinable() {
            // trace!("Reaping orphaned task... {:?}", self);
            let _exit_value = self.task.reap_exit_value();
            // trace!("Reaped orphaned task {:?}, {:?}", self, _exit_value);
        }
    }

    /// Perform any actions needed after a context switch.
    /// 
    /// Currently this only does two things:
    /// 1. Drops any data that the original previous task (before the context switch)
    ///    prepared for us to drop after the context switch has completed.
    /// 2. Obtains the preemption guard such that preemption can be re-enabled
    ///    when it is appropriate to do so.
    ///
    /// Note: this publicly re-exports the private `post_context_switch_action()`
    ///       function for use in the early `spawn::task_wrapper` functions,
    ///       which is the only place where an `ExitableTaskRef` can be obtained. 
    pub fn post_context_switch_action(&self) -> PreemptionGuard {
        post_context_switch_action()
    }

    /// Allows the unwinder to use the current task to obtain its `ExitableTaskRef`
    /// and its [`FailureCleanupFunction`] to be able to invoke it.
    #[doc(hidden)]
    pub fn obtain_for_unwinder(current_task: TaskRef) -> (Self, FailureCleanupFunction) {
        assert!(
            with_current_task(|t| t == &current_task).unwrap_or(false),
            "BUG: obtain_for_unwinder() invoked with a non-current task",
        );
        let func = current_task.0.failure_cleanup_function;
        (Self { task: current_task }, func)
    }
}


/// Registers a kill handler function for the current `Task`.
/// 
/// [`KillHandler`]s are called when a `Task` panics or otherwise fails
/// (e.g., due to a machine exception).
///
/// # Locking / Deadlock
/// Obtains the lock on this `Task`'s inner state in order to mutate it.
pub fn set_kill_handler(function: KillHandler) -> Result<(), &'static str> {
    with_current_task(|t| {
        t.0.task.inner().lock().kill_handler = Some(function);
    })
    .map_err(|_| "couldn't get current task")
}

/// Takes ownership of the current `Task`'s [`KillHandler`] function.
/// 
/// The registered `KillHandler` function is removed from the current task,
/// if it exists, and returned such that it can be invoked without holding
/// the `Task`'s inner lock.
/// 
/// After invoking this, the current task's kill handler will be `None`.
///
/// # Locking / Deadlock
/// Obtains the lock on this `Task`'s inner state in order to mutate it.
pub fn take_kill_handler() -> Option<KillHandler> {
    with_current_task(|t| t.0.task.inner().lock().kill_handler.take())
        .ok()
        .flatten()
}


pub use scheduler::*;
mod scheduler {
    use super::*;

    /// Yields the current CPU by selecting a new `Task` to run next,
    /// and then switches to that new `Task`.
    ///
    /// The new "next" `Task` to run will be selected by the currently-active
    /// scheduler policy.
    ///
    /// Preemption will be disabled while this function runs,
    /// but interrupts are not disabled because it is not necessary.
    ///
    /// ## Return
    /// * `true` if a new task was selected and switched to.
    /// * `false` if no new task was selected,
    ///    meaning the current task will continue running.
    #[doc(alias("yield"))]
    pub fn schedule() -> bool {
        let preemption_guard = preemption::hold_preemption();
        // If preemption was not previously enabled (before we disabled it above),
        // then we shouldn't perform a task switch here.
        if !preemption_guard.preemption_was_enabled() {
            // trace!("Note: preemption was disabled on CPU {}, skipping scheduler.", cpu::current_cpu());
            return false;
        }

        let cpu_id = preemption_guard.cpu_id();

        let Some(next_task) = (SELECT_NEXT_TASK_FUNC.load())(cpu_id.into_u8()) else {
            return false; // keep running the same current task
        };

        let (did_switch, recovered_preemption_guard) = task_switch(
            next_task,
            cpu_id,
            preemption_guard,
        ); 

        // trace!("AFTER TASK_SWITCH CALL (CPU {}) new current: {:?}, interrupts are {}", cpu_id, task::get_my_current_task(), irq_safety::interrupts_enabled());

        drop(recovered_preemption_guard);
        did_switch
    }

    /// The signature for the function that selects the next task for the given CPU.
    ///
    /// This is used when the [`schedule()`] function is invoked.
    pub type SchedulerFunc = fn(u8) -> Option<TaskRef>;

    /// The function currently registered as the system-wide scheduler policy.
    ///
    /// This is initialized to a dummy function that returns no "next" task,
    /// meaning that no scheduling will occur until it is initialized.
    /// Currently, this is initialized from within `scheduler::init()`.
    static SELECT_NEXT_TASK_FUNC: AtomicCell<SchedulerFunc> = AtomicCell::new(|_| None);

    /// Sets the active scheduler policy used by [`schedule()`] to select the next task.
    ///
    /// Currently, we only support one scheduler policy for the whole system,
    /// but supporting different policies on a per-CPU, per-namespace, or per-arbitrary domain basis
    /// would be a relatively simple immprovement.
    pub fn set_scheduler_policy(select_next_task_func: SchedulerFunc) {
        SELECT_NEXT_TASK_FUNC.store(select_next_task_func);
    }
}


/// Switches from the current task to the given `next` task.
///
/// ## Arguments
/// * `next`: the task to switch to.
/// * `cpu_id`: the ID of the current CPU.
/// * `preemption_guard`: a guard that is used to ensure preemption is disabled
///    for the duration of this task switch operation.
///
/// ## Important Note about Control Flow
/// If this is the first time that `next` task has been switched to,
/// the control flow will *NOT* return from this function,
/// and will instead jump to a wrapper function (that will directly invoke
/// the `next` task's entry point function.
///
/// Control flow may eventually return to this point, but not until another
/// task switch occurs away from the given `next` task to a different task.
/// Note that regardless of control flow, the return values will always be valid and correct.
///
/// ## Return
/// Returns a tuple of:
/// 1. a `bool` indicating whether an actual task switch occurred:
///    * If `true`, the task switch did occur, and `next` is now the current task.
///    * If `false`, the task switch did not occur, and the current task is unchanged.
/// 2. a [`PreemptionGuard`] that allows the caller to control for how long
///    preemption remains disabled, i.e., until the guard is dropped.
///
/// ## Locking / Deadlock
/// Obtains brief locks on both the current `Task`'s inner state and
/// the given `next` `Task`'s inner state in order to mutate them.
pub fn task_switch(
    next: TaskRef,
    cpu_id: CpuId,
    preemption_guard: PreemptionGuard,
) -> (bool, PreemptionGuard) {

    // We use the `with_current_task_and_value()` closure here in order to ensure that
    // the borrowed reference to the current task is guaranteed to be dropped
    // *before* the actual context switch operation occurs.
    let result = with_current_task_tls_slot_mut(
        |curr, p_guard| task_switch_inner(curr, next, cpu_id, p_guard),
        preemption_guard,
    );
    
    // Here, we're done accessing the curr and next tasks' states,
    // and if it was successful, we can proceed to the actual context switch.
    let values_for_context_switch = match result {
        Ok(Ok(stack_ptrs)) => stack_ptrs,
        Ok(Err(early_retval)) => return early_retval,
        Err(preemption_guard) => {
            // Here, the closure returned an error, meaning we couldn't get the current task
            return (false, preemption_guard); // keep running the same current task
        }
    };
    
    // debug!("task_switch [4]: prev sp: {:#X}, next sp: {:#X}", prev_task_saved_sp as usize, next_task_saved_sp);

    /// A macro that calls the given context switch routine with two arguments:
    /// a mutable pointer to the curr task's stack pointer, and the next task's stack pointer.
    macro_rules! call_context_switch {
        ($func:expr) => ({
            unsafe {
                $func(values_for_context_switch.0, values_for_context_switch.1);
            }
        });
    }

    // Now it's time to perform the actual context switch.
    // If `simd_personality` is NOT enabled, then we proceed as normal 
    // using the singular context_switch routine that matches the actual build target. 
    #[cfg(not(simd_personality))] {
        call_context_switch!(context_switch::context_switch);
    }
    // If `simd_personality` is enabled, all `context_switch*` routines are available,
    // which allows us to choose one based on whether the prev/next Tasks are SIMD-enabled.
    #[cfg(simd_personality)] {
        let (curr_simd, next_simd) = (values_for_context_switch.2, values_for_context_switch.3);
        match (curr_simd, next_simd) {
            (SimdExt::None, SimdExt::None) => {
                // log::warn!("SWITCHING from REGULAR to REGULAR task");
                call_context_switch!(context_switch::context_switch_regular);
            }

            (SimdExt::None, SimdExt::SSE)  => {
                // log::warn!("SWITCHING from REGULAR to SSE task");
                call_context_switch!(context_switch::context_switch_regular_to_sse);
            }
            
            (SimdExt::None, SimdExt::AVX)  => {
                // log::warn!("SWITCHING from REGULAR to AVX task");
                call_context_switch!(context_switch::context_switch_regular_to_avx);
            }

            (SimdExt::SSE, SimdExt::None)  => {
                // log::warn!("SWITCHING from SSE to REGULAR task");
                call_context_switch!(context_switch::context_switch_sse_to_regular);
            }

            (SimdExt::SSE, SimdExt::SSE)   => {
                // log::warn!("SWITCHING from SSE to SSE task");
                call_context_switch!(context_switch::context_switch_sse);
            }

            (SimdExt::SSE, SimdExt::AVX) => {
                // log::warn!("SWITCHING from SSE to AVX task");
                call_context_switch!(context_switch::context_switch_sse_to_avx);
            }

            (SimdExt::AVX, SimdExt::None) => {
                // log::warn!("SWITCHING from AVX to REGULAR task");
                call_context_switch!(context_switch::context_switch_avx_to_regular);
            }

            (SimdExt::AVX, SimdExt::SSE) => {
                log::warn!("SWITCHING from AVX to SSE task");
                call_context_switch!(context_switch::context_switch_avx_to_sse);
            }

            (SimdExt::AVX, SimdExt::AVX) => {
                // log::warn!("SWITCHING from AVX to AVX task");
                call_context_switch!(context_switch::context_switch_avx);
            }
        }
    }
    ///////////////////////////////////////////////////////////////////////////////////////////
    // *** Important Notes about Behavior after a Context Switch ***
    //
    // Here, after the actual context switch operation, the stacks have been switched.
    // Thus, `next` has become the current task.
    //
    // If this is **NOT** the first time the newly-current task has run,
    // then it will resume execution below as normal because this is where it left off
    // when the context switch operation occurred.
    //
    // However, if this **is** the first time that the newly-current task
    // has been switched to and is running, the control flow will **NOT** proceed here.
    // Instead, it will have directly jumped to its entry point, i.e.,`spawn::task_wrapper()`.
    //
    // As such, anything we do below must also be done in `spawn::task_wrapper()`.
    // Thus, we want to ensure that post-context switch actions below are kept minimal
    // and are easy to replicate in `task_wrapper()`.
    ///////////////////////////////////////////////////////////////////////////////////////////

    let recovered_preemption_guard = post_context_switch_action();
    (true, recovered_preemption_guard)
}

#[cfg(not(simd_personality))]
type TaskSwitchInnerRet = (*mut usize, usize);
#[cfg(simd_personality)]
type TaskSwitchInnerRet = (*mut usize, usize, SimdExt, SimdExt);

/// The inner part of the task switching routine that modifies task states.
///
/// This accepts a mutably-borrowed reference to the current task's TLS variable
/// in order to potentially deinit that TLS variable if the current task has exited.
/// Thus, it cannot perform the actual context switch operation because we cannot
/// context switch until all `TaskRef`s on the current stack are dropped.
/// Hence, the the main [`task_switch()`] routine proceeds with the context switch
/// after we return to it from this function.
fn task_switch_inner(
    mut curr_task_tls_slot: RefMut<'_, Option<TaskRef>>,
    next: TaskRef,
    cpu_id: CpuId,
    preemption_guard: PreemptionGuard,
) -> Result<TaskSwitchInnerRet, (bool, PreemptionGuard)> {
    let Some(curr) = curr_task_tls_slot.as_ref() else {
        error!("BUG: task_switch_inner(): couldn't get current task");
        return Err((false, preemption_guard));
    };

    // No need to task switch if the next task is the same as the current task.
    if curr.id == next.id {
        return Err((false, preemption_guard));
    }

    // log::trace!("task_switch [0]: (CPU {}) prev {:?}, next {:?}, interrupts?: {}", cpu_id, curr, next, irq_safety::interrupts_enabled());

    // These conditions are checked elsewhere, but can be re-enabled if we want to be extra strict.
    // if !next.is_runnable() {
    //     error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was not Runnable! Current: {:?}, Next: {:?}", curr, next);
    //     return (false, preemption_guard);
    // }
    // if next.is_running() {
    //     error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was already running on CPU {}!\nCurrent: {:?} Next: {:?}", cpu_id, curr, next);
    //     return (false, preemption_guard);
    // }
    // if let Some(pc) = next.pinned_cpu() {
    //     if pc != cpu_id {
    //         error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was pinned to CPU {:?} but scheduled on CPU {}!\n\tCurrent: {:?}, Next: {:?}", pc, cpu_id, curr, next);
    //         return (false, preemption_guard);
    //     }
    // }

    // Note that because userspace support is currently disabled, this will never happen.
    // // Change the privilege stack (RSP0) in the TSS.
    // // We can safely skip setting the TSS RSP0 when switching to a kernel task, 
    // // i.e., when `next` is not a userspace task.
    // if next.is_userspace() {
    //     let (stack_bottom, stack_size) = {
    //         let kstack = &next.task.0.task.inner().lock().kstack;
    //         (kstack.bottom(), kstack.size_in_bytes())
    //     };
    //     let new_tss_rsp0 = stack_bottom + (stack_size / 2); // the middle half of the stack
    //     if tss::tss_set_rsp0(new_tss_rsp0).is_ok() { 
    //         // debug!("task_switch [2]: new_tss_rsp = {:#X}", new_tss_rsp0);
    //     } else {
    //         error!("task_switch(): failed to set CPU {} TSS RSP0, aborting task switch!", cpu_id);
    //         return (false, preemption_guard);
    //     }
    // }

    // // Switch page tables. 
    // // Since there is only a single address space (as userspace support is currently disabled),
    // // we do not need to do this at all.
    // if false {
    //     let prev_mmi = &curr.mmi;
    //     let next_mmi = &next.mmi;
    //    
    //     if Arc::ptr_eq(prev_mmi, next_mmi) {
    //         // do nothing because we're not changing address spaces
    //         // debug!("task_switch [3]: prev_mmi is the same as next_mmi!");
    //     } else {
    //         // time to change to a different address space and switch the page tables!
    //         let mut prev_mmi_locked = prev_mmi.lock();
    //         let next_mmi_locked = next_mmi.lock();
    //         // debug!("task_switch [3]: switching tables! From {} {:?} to {} {:?}", 
    //         //         curr.name, prev_mmi_locked.page_table, next.name, next_mmi_locked.page_table);
    //
    //         prev_mmi_locked.page_table.switch(&next_mmi_locked.page_table);
    //     }
    // }

    let prev_task_saved_sp: *mut usize = {
        let mut inner = curr.0.task.inner().lock(); // ensure the lock is released
        (&mut inner.saved_sp) as *mut usize
    };
    let next_task_saved_sp: usize = {
        let inner = next.0.task.inner().lock(); // ensure the lock is released
        inner.saved_sp
    };

    // Mark the current task as no longer running
    curr.0.task.running_on_cpu().store(None.into());

    // After this point, we may need to mutate the `curr_task_tls_slot` (if curr has exited),
    // so we use local variables to store some necessary info about the curr task
    // and then end our immutable borrow of the current task.
    let curr_task_has_exited = curr.has_exited();
    #[cfg(simd_personality)]
    let curr_simd = curr.simd;

    // If the current task has exited at this point, then it will never run again.
    // Thus, we need to remove or "deinit" the `TaskRef` in its TLS area
    // in order to ensure that its `TaskRef` reference count will be decremented properly
    // and thus its task struct will eventually be dropped.
    // We store the removed `TaskRef` in CPU-local storage so that it remains accessible
    // until *after* the context switch.
    if curr_task_has_exited {
        // log::trace!("[CPU {}] task_switch(): deiniting current task TLS for: {:?}, next: {}", cpu_id, curr_task_tls_slot.as_deref(), next.deref());
        let prev_taskref = curr_task_tls_slot.take();
        DROP_AFTER_TASK_SWITCH.set_guarded(prev_taskref, &preemption_guard);
    }

    // Now we are done touching the current task's TLS slot, so proactively drop it now
    // to ensure that it isn't accidentally dropped later after we've switched the active TLS area.
    drop(curr_task_tls_slot);

    // Now, set the next task as the current task running on this CPU.
    //
    // Note that we cannot do this until we've done the above part that cleans up
    // TLS variables for the current task (if exited), since the below call to 
    // `set_as_current_task()` will change the currently active TLS area on this CPU.
    //
    // We briefly disable interrupts below to ensure that any interrupt handlers that may run
    // on this CPU during the schedule/task_switch routines cannot observe inconsistencies
    // in task runstates, e.g., when an interrupt handler accesses the current task context.
    {
        let _held_interrupts = hold_interrupts();
        next.0.task.running_on_cpu().store(Some(cpu_id).into());
        next.set_as_current_task();
        drop(_held_interrupts);
    }

    // Move the preemption guard into CPU-local storage such that we can retrieve it
    // after the actual context switch operation has completed.
    TASK_SWITCH_PREEMPTION_GUARD.set(preemption_guard);

    #[cfg(not(simd_personality))]
    return Ok((prev_task_saved_sp, next_task_saved_sp));
    #[cfg(simd_personality)]
    return Ok((prev_task_saved_sp, next_task_saved_sp, curr_simd, next.simd));
}

/// Perform any actions needed after a context switch.
///
/// Currently this only does two things:
/// 1. Drops any data that the original previous task (before the context switch)
///    prepared for us to drop.
/// 2. Obtains the preemption guard such that preemption can be re-enabled
///    when it is appropriate to do so.
fn post_context_switch_action() -> PreemptionGuard {
    let guard_1 = preemption::hold_preemption();
    let guard_2 = TASK_SWITCH_PREEMPTION_GUARD
        .replace_guarded(None, &guard_1)
        .expect("BUG: post_context_switch_action: no PreemptionGuard existed");
    // Doesn't really matter which guard we use.
    DROP_AFTER_TASK_SWITCH.set_guarded(None, &guard_2);
    guard_2
}


/// The preemption guard that was used for safe task switching on each CPU.
///
/// The `PreemptionGuard` is stored here right before a context switch begins
/// and then retrieved from here right after the context switch ends.
/// It is stored in a CPU-local variable because it's only related to
/// a task switching operation on a particular CPU.
#[cls::cpu_local(stores_guard = PreemptionGuard)]
static TASK_SWITCH_PREEMPTION_GUARD: Option<PreemptionGuard> = None;

/// Data that should be dropped after switching away from a task that has exited.
///
/// Currently, this contains the previous Task's `TaskRef` removed from its TLS area;
/// it is stored in a CPU-local variable because it's only related to
/// a task switching operation on a particular CPU.
#[cls::cpu_local]
static DROP_AFTER_TASK_SWITCH: Option<TaskRef> = None;

pub use tls_current_task::*;
/// A private module to ensure the below TLS variables aren't modified directly.
mod tls_current_task {
    use core::{cell::{Cell, RefCell}, ops::Deref};
    use super::{TASKLIST, TaskRef, ExitableTaskRef};

    /// The TLS area that holds the current task's ID.
    #[thread_local]
    static CURRENT_TASK_ID: Cell<usize> = Cell::new(0);

    /// The TLS area that holds the current task.
    #[thread_local]
    static CURRENT_TASK: RefCell<Option<TaskRef>> = RefCell::new(None);

    /// Invokes the given `function` with a reference to the current task.
    ///
    /// This is useful to avoid cloning a reference to the current task.
    ///
    /// Returns a `CurrentTaskNotFound` error if the current task cannot be obtained.
    pub fn with_current_task<F, R>(function: F) -> Result<R, CurrentTaskNotFound>
    where
        F: FnOnce(&TaskRef) -> R
    {
        if let Ok(Some(ref t)) = CURRENT_TASK.try_borrow().as_deref() {
            Ok(function(t))
        } else {
            Err(CurrentTaskNotFound)
        }
    }

    /// Similar to [`with_current_task()`], but also accepts a value that is
    /// passed to the given `function` or returned in the case of an error.
    ///
    /// This is useful for two reasons:
    /// 1. Like [`with_current_task()`], it avoids cloning a reference to the current task.
    /// 2. It allows the `value` to be returned upon an error, instead of the behavior
    ///    in [`with_current_task()`] that unconditionally takes ownership of the `value`
    ///    without any way to recover ownership of that `value`.
    ///
    /// Returns an `Err` containing the `value` if the current task cannot be obtained.
    pub fn with_current_task_and_value<F, R, T>(function: F, value: T) -> Result<R, T>
    where
        F: FnOnce(&TaskRef, T) -> R
    {
        if let Ok(Some(ref t)) = CURRENT_TASK.try_borrow().as_deref() {
            Ok(function(t, value))
        } else {
            Err(value)
        }
    }

    /// Returns a cloned reference to the current task.
    ///
    /// Using [`with_current_task()`] is preferred because it operates on a
    /// borrowed reference to the current task and avoids cloning that reference.
    ///
    /// This function must clone the current task's `TaskRef` in order to ensure
    /// that this task cannot be dropped for the lifetime of the returned `TaskRef`.
    /// Because the "current task" feature uses thread-local storage (TLS),
    /// there is no safe way to avoid the cloning operation because it is impossible
    /// to specify the lifetime of the returned thread-local reference in Rust.
    pub fn get_my_current_task() -> Option<TaskRef> {
        with_current_task(|t| t.clone()).ok()
    }

    /// Returns the unique ID of the current task.
    pub fn get_my_current_task_id() -> usize {
        CURRENT_TASK_ID.get()
    }

    /// Initializes the TLS variable(s) used for tracking the "current" task.
    ///
    /// This function being public is completely safe, as it will only ever execute
    /// once per task, typically at the beginning of a task's first execution.
    ///
    /// If `current_task` is `Some`, its task ID must match `current_task_id`.
    /// If `current_task` is `None`, the task must have already been added to 
    /// the system-wide task list such that a reference to it can be retrieved.
    ///
    /// # Return
    /// * On success, an [`ExitableTaskRef`] for the current task,
    ///   which can only be obtained once at the very start of the task's execution,
    ///   and only from this one function. 
    /// * Returns an `Err` if the current task has already been initialized.
    #[doc(hidden)]
    pub fn init_current_task(
        current_task_id: usize,
        current_task: Option<TaskRef>,
    ) -> Result<ExitableTaskRef, InitCurrentTaskError> {
        let taskref = if let Some(t) = current_task {
            if t.id != current_task_id {
                log::error!("BUG: `current_task` {:?} did not match `current_task_id` {}",
                    t, current_task_id
                );
                return Err(InitCurrentTaskError::MismatchedTaskIds(current_task_id, t.id));
            }
            t
        } else {
            TASKLIST.lock()
                .get(&current_task_id)
                .cloned()
                .ok_or_else(|| {
                    log::error!("Couldn't find current_task_id {} in TASKLIST", current_task_id);
                    InitCurrentTaskError::NotInTasklist(current_task_id)
                })?
        };

        match CURRENT_TASK.try_borrow_mut() {
            Ok(mut t_opt) => if let Some(_existing_task) = t_opt.deref() {
                log::error!("BUG: init_current_task(): CURRENT_TASK was already `Some()`");
                log::error!("  --> attemping to dump existing task: {:?}", _existing_task);
                Err(InitCurrentTaskError::AlreadyInited(_existing_task.id))
            } else {
                *t_opt = Some(taskref.clone());
                CURRENT_TASK_ID.set(current_task_id);
                Ok(ExitableTaskRef { task: taskref })
            }
            Err(_e) => {
                log::error!("[CPU {}] BUG: init_current_task(): failed to mutably borrow CURRENT_TASK. \
                    Task ID: {}, {:?}", cpu::current_cpu(), current_task_id, taskref,
                );
                Err(InitCurrentTaskError::AlreadyBorrowed(current_task_id))
            }
        }
    }

    /// An internal routine that exposes mutable access to the current task's TLS variable.
    ///
    /// This mutable access to the TLS variable is only needed for task switching,
    /// in which an exited task must clean up its current task TLS variable.
    ///
    /// Otherwise, it is similar to [`with_current_task_and_value()`].
    ///
    /// Returns an `Err` containing the `value` if the current task cannot be obtained.
    pub(crate) fn with_current_task_tls_slot_mut<F, R, T>(function: F, value: T) -> Result<R, T>
    where
        F: FnOnce(core::cell::RefMut<'_, Option<TaskRef>>, T) -> R
    {
        if let Ok(tls_slot) = CURRENT_TASK.try_borrow_mut() {
            Ok(function(tls_slot, value))
        } else {
            Err(value)
        }
    }

    /// An error type indicating that the current task was already initialized.
    #[derive(Debug)]
    pub enum InitCurrentTaskError {
        /// The task IDs used as arguments to `init_current_task()` did not match.
        MismatchedTaskIds(usize, usize),
        /// The enclosed Task ID was not in the system-wide task list.
        NotInTasklist(usize),
        /// The current task was already initialized; its task ID is enclosed.
        AlreadyInited(usize),
        /// The current task reference was already borrowed, thus it could not be
        /// mutably borrowed again. The ID of the task attempting to be initialized is enclosed.
        AlreadyBorrowed(usize),

    }
    /// An error type indicating that the current task has not yet been initialized.
    #[derive(Debug)]
    pub struct CurrentTaskNotFound;
}


/// Bootstraps a new task from the current thread of execution.
///
/// Returns a tuple of:
/// 1. a [`JoinableTaskRef`], which allows another task to join this bootstrapped task,
/// 2. an [`ExitableTaskRef`], which allows this bootstrapped task to mark itself
///    as exited once it has completed running.
///
/// ## Note
/// This function does not add the new task to any runqueue.
pub fn bootstrap_task(
    cpu_id: CpuId, 
    stack: NoDrop<Stack>,
    kernel_mmi_ref: MmiRef,
) -> Result<(JoinableTaskRef, ExitableTaskRef), &'static str> {
    let namespace = mod_mgmt::get_initial_kernel_namespace()
        .ok_or("Must initalize kernel CrateNamespace (mod_mgmt) before the tasking subsystem.")?
        .clone();
    let env = Arc::new(Mutex::new(Environment::default()));
    let mut bootstrap_task = Task::new(
        Some(stack.into_inner()),
        InheritedStates::Custom {
            mmi: kernel_mmi_ref,
            namespace,
            env,
            app_crate: None,
        },
    )?;
    bootstrap_task.name = format!("bootstrap_task_cpu_{cpu_id}");
    let bootstrap_task_id = bootstrap_task.id;
    let joinable_taskref = TaskRef::create(
        bootstrap_task,
        bootstrap_task_cleanup_failure,
    );
    // Update other relevant states for this new bootstrapped task.
    joinable_taskref.0.task.runstate().store(RunState::Runnable);
    joinable_taskref.0.task.running_on_cpu().store(Some(cpu_id).into()); 
    joinable_taskref.0.task.inner().lock().pinned_cpu = Some(cpu_id); // can only run on this CPU core
    // Set this task as this CPU's current task, as it's already running.
    joinable_taskref.set_as_current_task();
    let exitable_taskref = match init_current_task(
        bootstrap_task_id,
        Some(joinable_taskref.clone())
    ) {
        Ok(t) => t,
        Err(e) => {
            error!("BUG: failed to set boostrapped task as current task on CPU {}, {:?}",
                cpu_id, e
            );
            // Don't drop the bootstrap task upon error, because it contains the stack
            // used for the currently running code -- that would trigger an exception.
            let _task_ref = NoDrop::new(joinable_taskref);
            return Err("BUG: bootstrap_task(): failed to set bootstrapped task as current task");
        }
    };
    Ok((joinable_taskref, exitable_taskref))
}


/// This is just like `spawn::task_cleanup_failure()`,
/// but for the initial tasks bootstrapped from each CPU's first execution context.
///
/// However, for a bootstrapped task, we don't know its function signature, argument type,
/// or return value type because it was invoked from assembly and doesn't really have one.
///
/// Therefore there's not much we can actually do besides log an error and spin.
fn bootstrap_task_cleanup_failure(current_task: ExitableTaskRef, kill_reason: KillReason) -> ! {
    error!("BUG: bootstrap_task_cleanup_failure: {:?} died with {:?}\n. \
        There's nothing we can do here; looping indefinitely!",
        current_task,
        kill_reason,
    );
    // If an initial bootstrap task fails, there's nothing else we can do.
    loop { 
        core::hint::spin_loop();
    }
}
