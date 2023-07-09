//! This crate contains the basic [`Task`] structure, which holds contextual execution states
//! needed to support safe multithreading.
//! 
//! To create new `Task`s, use the [`spawn`](../spawn/index.html) crate.
//! For more advanced task-related types, see the [`task`](../task/index.html) crate.

#![no_std]
#![feature(panic_info_message)]
#![feature(negative_impls)]
#![allow(clippy::type_complexity)]

extern crate alloc;

use core::{
    any::Any,
    fmt,
    hash::{Hash, Hasher},
    ops::Deref,
    panic::PanicInfo,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    task::Waker,
};
use alloc::{
    boxed::Box,
    format,
    string::String,
    sync::Arc,
};
use cpu::{CpuId, OptionalCpuId};
use crossbeam_utils::atomic::AtomicCell;
use sync_irq::IrqSafeMutex;
use log::{warn, trace};
use memory::MmiRef;
use stack::Stack;
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use mod_mgmt::{AppCrateRef, CrateNamespace, TlsDataImage};
use environment::Environment;
use spin::Mutex;

/// The function signature of the callback that will be invoked when a `Task`
/// panics or otherwise fails, e.g., a machine exception occurs.
pub type KillHandler = Box<dyn Fn(&KillReason) + Send>;

/// Just like `core::panic::PanicInfo`, but with owned String types instead of &str references.
#[derive(Debug, Default)]
pub struct PanicInfoOwned {
    pub payload:  Option<Box<dyn Any + Send>>,
    pub msg:      String,
    pub file:     String,
    pub line:     u32, 
    pub column:   u32,
}
impl fmt::Display for PanicInfoOwned {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}:{}:{} -- {:?}", self.file, self.line, self.column, self.msg)
    }
}
impl<'p> From<&PanicInfo<'p>> for PanicInfoOwned {
    fn from(info: &PanicInfo) -> PanicInfoOwned {
        let msg = info.message()
            .map(|m| format!("{m}"))
            .unwrap_or_default();
        let (file, line, column) = if let Some(loc) = info.location() {
            (String::from(loc.file()), loc.line(), loc.column())
        } else {
            (String::new(), 0, 0)
        };

        PanicInfoOwned { payload: None, msg, file, line, column }
    }
}
impl PanicInfoOwned {
    /// Constructs a new `PanicInfoOwned` object containing only the given `payload`
    /// without any location or message info.
    /// 
    /// Useful for forwarding panic payloads through a catch and resume unwinding sequence.
    pub fn from_payload(payload: Box<dyn Any + Send>) -> PanicInfoOwned {
        PanicInfoOwned {
            payload: Some(payload),
            ..Default::default()
        }
    }
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
        match self {
            Self::Requested         => write!(f, "Requested"),
            Self::Panic(panic_info) => write!(f, "Panicked at {panic_info}"),
            Self::Exception(num)    => write!(f, "Exception {num:#X}({num})"),
        }
    }
}


/// The two ways a `Task` can exit, including possible return values and conditions.
#[derive(Debug)]
pub enum ExitValue {
    /// The `Task` ran to completion
    /// and returned the enclosed [`Any`] value from its entry point function.
    ///
    /// The caller of this task's entry point function should know which concrete type
    /// this Task returned, and is thus able to downcast it appropriately.
    Completed(Box<dyn Any + Send>),
    /// The `Task` did NOT run to completion but was instead killed for the enclosed reason.
    Killed(KillReason),
}


/// The set of possible runstates that a `Task` can be in.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RunState {
    /// This task is in the midst of being initialized/spawned.
    Initing,
    /// This task is able to be scheduled in, but not necessarily currently running.
    /// To check whether it is currently running, use [`Task::is_running()`].
    Runnable,
    /// This task is blocked on something and is *not* able to be scheduled in.
    Blocked,
    /// This `Task` has exited and can no longer be run.
    /// This covers both the case when a task ran to completion or was killed;
    /// see [`ExitValue`] for more details.
    Exited,
    /// This `Task` had already exited, and now its [`ExitValue`] has been taken
    /// (either by another task that `join`ed it, or by the system).
    /// Because a task's exit value can only be taken once, a repaed task
    /// is useless and will be cleaned up and removed from the system.
    Reaped,
}


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

/// A struct holding data items needed to restart a `Task`.
pub struct RestartInfo {
    /// Stores the argument of the task for restartable tasks
    pub argument: Box<dyn Any + Send>,
    /// Stores the function of the task for restartable tasks
    pub func: Box<dyn Any + Send>,
}


/// The parts of a `Task` that may be modified after its creation.
///
/// This includes only the parts that cannot be modified atomically.
/// As such, they are protected by a lock in the containing `Task` struct.
///
/// In general, other crates cannot obtain a mutable reference to a Task (`&mut Task`),
/// which means they cannot access this struct's contents directly at all 
/// (except through the specific get/set methods exposed by `Task`).
///
/// Therefore, it is safe to expose all members of this struct as public, 
/// though not strictly necessary. 
/// Currently, we only publicize the fields here that need to be modified externally,
/// primarily by the `spawn` and `task` crates for creating and running new tasks. 
pub struct TaskInner {
    /// the saved stack pointer value, used for task switching.
    pub saved_sp: usize,
    /// The kernel stack, which all `Task`s must have in order to execute.
    pub kstack: Stack,
    /// Whether or not this task is pinned to a certain CPU.
    /// The idle tasks are always pinned to their respective CPU.
    pub pinned_cpu: Option<CpuId>,
    /// The function that will be called when this `Task` panics or fails due to a machine exception.
    /// It will be invoked before the task is cleaned up via stack unwinding.
    /// This is similar to Rust's built-in panic hook, but is also called upon a machine exception, not just a panic.
    pub kill_handler: Option<KillHandler>,
    /// The environment variables for this task, which are shared among child and parent tasks by default.
    env: Arc<Mutex<Environment>>,
    /// Stores the restartable information of the task. 
    /// `Some(RestartInfo)` indicates that the task is restartable.
    pub restart_info: Option<RestartInfo>,
    /// The waker that is awoken when this task completes.
    pub waker: Option<Waker>,
}


/// A structure that contains contextual information for a thread of execution. 
///
/// # Implementation note
/// Only fields that do not permit interior mutability can safely be exposed as public
/// because we allow foreign crates to directly access task struct fields.
pub struct Task {
    /// The mutable parts of a `Task` struct that can be modified after task creation,
    /// excluding private items that can be modified atomically.
    ///
    /// We use this inner structure to reduce contention when accessing task struct fields,
    /// because the other fields aside from this one are primarily read, not written.
    ///
    /// This must not be public because it permits interior mutability of key task states.
    inner: IrqSafeMutex<TaskInner>,

    /// The unique identifier of this Task.
    pub id: usize,
    /// The simple name of this Task.
    pub name: String,
    /// Which cpu core this Task is currently running on;
    /// `None` if not currently running.
    /// We use `OptionalCpuId` instead of `Option<CpuId>` to ensure that 
    /// this field is accessed using lock-free native atomic instructions.
    ///
    /// This is not public because it permits interior mutability.
    running_on_cpu: AtomicCell<OptionalCpuId>,
    /// The runnability of this task, i.e., whether it's eligible to be scheduled in.
    ///
    /// This is not public because it permits interior mutability.
    runstate: AtomicCell<RunState>,
    /// Whether the task is suspended.
    ///
    /// This is only triggered by a Ctrl + Z in the terminal.
    ///
    /// This is not public because it permits interior mutability.
    suspended: AtomicBool,
    /// Memory management details: page tables, mappings, allocators, etc.
    /// This is shared among all other tasks in the same address space.
    pub mmi: MmiRef, 
    /// Whether this Task is an idle task, the task that runs by default when no other task is running.
    /// There exists one idle task per core, so this is `false` for most tasks.
    pub is_an_idle_task: bool,
    /// For application `Task`s, this is effectively a reference to the [`mod_mgmt::LoadedCrate`]
    /// that contains the entry function for this `Task`.
    pub app_crate: Option<Arc<AppCrateRef>>,
    /// This `Task` is linked into and runs within the context of this [`CrateNamespace`].
    pub namespace: Arc<CrateNamespace>,
    /// The Thread-Local Storage (TLS) area for this task.
    ///
    /// Upon each task switch, we must set the value of the TLS base register 
    /// (e.g., FsBase on x86_64) to the value of this TLS area's self pointer.
    tls_area: TlsDataImage,
    
    #[cfg(simd_personality)]
    /// Whether this Task is SIMD enabled and what level of SIMD extensions it uses.
    pub simd: SimdExt,
}

// Ensure that atomic fields in the `Tast` struct are actually lock-free atomics.
const _: () = assert!(AtomicCell::<OptionalCpuId>::is_lock_free());
const _: () = assert!(AtomicCell::<RunState>::is_lock_free());

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut ds = f.debug_struct("Task");
        ds.field("name", &self.name)
            .field("id", &self.id)
            .field("running_on", &self.running_on_cpu())
            .field("runstate", &self.runstate());
        if let Some(inner) = self.inner.try_lock() {
            ds.field("pinned", &inner.pinned_cpu);
        } else {
            ds.field("pinned", &"<Locked>");
        }
        ds.finish()
    }
}
impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{{{}}}", self.name, self.id)
    }
}
impl Hash for Task {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.id.hash(h);
    }
}

impl Task {
    /// Creates a new `Task` and initializes it to be non-`Runnable`.
    ///
    /// # Arguments
    /// * `stack`: the optional `Stack` for this new `Task` to use.
    ///    * If `None`, a stack of the default size will be allocated and used.
    /// * `inherited states`: the set of states used to initialize this new `Task`.
    ///    * Typically, a caller will pass in [`InheritedStates::FromTask`] with the
    ///      enclosed task being a reference to the current task.
    ///      In this way, the enclosed task acts as a sort of "parent" template
    ///      for this new `Task`.
    ///      Theseus doesn't have a true parent-child relationship between tasks;
    ///      the new `Task` merely inherits select states from it.
    ///
    /// # Usage Notes
    /// * This does not run the task, schedule it in, or switch to it.
    /// * If you want to create a new task, you should use the `spawn` crate instead.
    pub fn new(
        stack: Option<Stack>,
        states_to_inherit: InheritedStates,
    ) -> Result<Task, &'static str> {
        /// The counter of task IDs. We start at `1` such that `0` can be used 
        /// as a task ID that indicates the absence of a task, e.g., in sync primitives. 
        static TASKID_COUNTER: AtomicUsize = AtomicUsize::new(1);

        let (mmi, namespace, env, app_crate) = states_to_inherit.into_tuple();
        let kstack = stack
            .or_else(|| stack::alloc_stack(KERNEL_STACK_SIZE_IN_PAGES, &mut mmi.lock().page_table))
            .ok_or("couldn't allocate stack for new Task!")?;

        // TODO: re-use old task IDs again, instead of simply blindly counting up.
        let task_id = TASKID_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Obtain a new copied instance of the TLS data image for this task.
        let tls_area = namespace.get_tls_initializer_data();

        Ok(Task {
            inner: IrqSafeMutex::new(TaskInner {
                saved_sp: 0,
                kstack,
                pinned_cpu: None,
                kill_handler: None,
                env,
                restart_info: None,
                waker: None,
            }),
            id: task_id,
            name: format!("task_{task_id}"),
            running_on_cpu: AtomicCell::new(None.into()),
            runstate: AtomicCell::new(RunState::Initing),
            suspended: AtomicBool::new(false),
            mmi,
            is_an_idle_task: false,
            app_crate,
            namespace,
            tls_area,

            #[cfg(simd_personality)]
            simd: SimdExt::None,
        })
    }

    /// Sets the `Environment` of this Task.
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on this `Task`'s inner state in order to mutate it.
    pub fn set_env(&self, new_env: Arc<Mutex<Environment>>) {
        self.inner.lock().env = new_env;
    }

    /// Gets a reference to this task's `Environment`.
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on this `Task`'s inner state in order to access it.
    pub fn get_env(&self) -> Arc<Mutex<Environment>> {
        Arc::clone(&self.inner.lock().env)
    }

    /// Returns `true` if this `Task` is currently running.
    pub fn is_running(&self) -> bool {
        self.running_on_cpu().is_some()
    }

    /// Returns the ID of the CPU this `Task` is currently running on.
    pub fn running_on_cpu(&self) -> Option<CpuId> {
        self.running_on_cpu.load().into()
    }

    /// Returns the ID of the CPU this `Task` is pinned on,
    /// or `None` if it is not pinned.
    pub fn pinned_cpu(&self) -> Option<CpuId> {
        self.inner.lock().pinned_cpu
    }

    /// Returns the current [`RunState`] of this `Task`.
    pub fn runstate(&self) -> RunState {
        self.runstate.load()
    }

    /// Returns whether this `Task` is runnable, i.e., able to be scheduled in.
    ///
    /// For this to return `true`, this `Task`'s runstate must be [`Runnable`]
    /// and it must not be [suspended].
    ///
    /// # Note
    /// This does *NOT* mean that this `Task` is actually currently [running],
    /// just that it is *able* to be run.
    ///
    /// [`Runnable`]: RunState::Runnable
    /// [suspended]: Task::is_suspended
    /// [running]: Task::is_running
    pub fn is_runnable(&self) -> bool {
        self.runstate() == RunState::Runnable && !self.is_suspended()
    }

    /// Returns the namespace that this `Task` is loaded/linked into and runs within.
    pub fn get_namespace(&self) -> &Arc<CrateNamespace> {
        &self.namespace
    }

    /// Exposes read-only access to this `Task`'s [`Stack`] by invoking
    /// the given `func` with a reference to its kernel stack.
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on this `Task`'s inner state for the duration of `func`
    /// in order to access its stack.
    /// The given `func` **must not** attempt to obtain that same inner lock.
    pub fn with_kstack<R, F>(&self, func: F) -> R 
        where F: FnOnce(&Stack) -> R
    {
        func(&self.inner.lock().kstack)
    }

    /// Returns a mutable reference to this `Task`'s inner state. 
    ///
    /// # Note about mutability
    /// This function requires the caller to have a mutable reference to this `Task`
    /// in order to protect the inner state from foreign crates accessing it
    /// through a `TaskRef` auto-dereferencing into a `Task`.
    /// This is because you can only obtain a mutable reference to a `Task`
    /// *before* you enclose it in a `TaskRef` wrapper type.
    ///
    /// Because this function requires a mutable reference to this `Task`,
    /// no locks must be obtained. 
    pub fn inner_mut(&mut self) -> &mut TaskInner {
        self.inner.get_mut()
    }

    /// Invokes `func` with immutable access to this `Task`'s [`RestartInfo`].
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on this `Task`'s inner state for the duration of `func`
    /// in order to access its stack.
    /// The given `func` **must not** attempt to obtain that same inner lock.
    pub fn with_restart_info<R, F>(&self, func: F) -> R 
        where F: FnOnce(Option<&RestartInfo>) -> R
    {
        func(self.inner.lock().restart_info.as_ref())
    }

    /// Returns `true` if this `Task` has been exited, i.e.,
    /// if its `RunState` is either `Exited` or `Reaped`.
    pub fn has_exited(&self) -> bool {
        matches!(self.runstate(), RunState::Exited | RunState::Reaped)
    }

    /// Returns `true` if this is an application `Task`.
    ///
    /// This will also return `true` if this task was spawned by an application task,
    /// since a task inherits the "application crate" field from its "parent" that spawned it.
    pub fn is_application(&self) -> bool {
        self.app_crate.is_some()
    }

    /// Returns `true` if this `Task` was spawned as a restartable task.
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on this `Task`'s inner state in order to access it.
    pub fn is_restartable(&self) -> bool {
        self.inner.lock().restart_info.is_some()
    }

    /// Blocks this `Task` by setting its runstate to [`RunState::Blocked`].
    ///
    /// Returns the previous runstate on success, and the current runstate on error.
    /// This will only succeed if the task is runnable or already blocked.
    pub fn block(&self) -> Result<RunState, RunState> {
        use RunState::{Blocked, Runnable};

        if self.runstate.compare_exchange(Runnable, Blocked).is_ok() {
            Ok(Runnable)
        } else if self.runstate.compare_exchange(Blocked, Blocked).is_ok() {
            warn!("Blocked an already blocked task: {:?}", self);
            Ok(Blocked)
        } else {
            Err(self.runstate.load())
        }
    }

    /// Blocks this `Task` if it is a newly-spawned task currently being initialized.
    ///
    /// This is a special case only to be used when spawning a new task that
    /// should not be immediately scheduled in; it will fail for all other cases.
    ///
    /// Returns the previous runstate (i.e. `RunState::Initing`) on success,
    /// or the current runstate on error.
    pub fn block_initing_task(&self) -> Result<RunState, RunState> {
        if self.runstate.compare_exchange(RunState::Initing, RunState::Blocked).is_ok() {
            Ok(RunState::Initing)
        } else {
            Err(self.runstate.load())
        }
    }

    /// Unblocks this `Task` by setting its runstate to [`RunState::Runnable`].
    ///
    /// Returns the previous runstate on success, and the current runstate on
    /// error. Will only succed if the task is blocked or already runnable.
    pub fn unblock(&self) -> Result<RunState, RunState> {
        use RunState::{Blocked, Runnable};

        if self.runstate.compare_exchange(Blocked, Runnable).is_ok() {
            Ok(Blocked)
        } else if self.runstate.compare_exchange(Runnable, Runnable).is_ok() {
            warn!("Unblocked an already runnable task: {:?}", self);
            Ok(Runnable)
        } else {
            Err(self.runstate.load())
        }
    }
    
    /// Makes this `Task` `Runnable` if it is a newly-spawned and fully initialized task.
    ///
    /// This is a special case only to be used when spawning a new task that
    /// is ready to be scheduled in; it will fail for all other cases.
    ///
    /// Returns the previous runstate (i.e. `RunState::Initing`) on success, and
    /// the current runstate on error.
    pub fn make_inited_task_runnable(&self) -> Result<RunState, RunState> {
        if self.runstate.compare_exchange(RunState::Initing, RunState::Runnable).is_ok() {
            Ok(RunState::Initing)
        } else {
            Err(self.runstate.load())
        }
    }

    /// Suspends this `Task`.
    pub fn suspend(&self) {
        self.suspended.store(true, Ordering::Release);
    }

    /// Unsuspends this `Task`.
    pub fn unsuspend(&self) {
        self.suspended.store(false, Ordering::Release);
    }

    /// Returns `true` if this `Task` is suspended.
    ///
    /// Note that a task being suspended is independent from its [`RunState`].
    pub fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::Acquire)
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        #[cfg(not(rq_eval))]
        trace!("[CPU {}] Task::drop(): {}", cpu::current_cpu(), self);

        // We must consume/drop the Task's kill handler BEFORE a Task can possibly be dropped.
        // This is because if an application task sets a kill handler that is a closure/function in the text section of the app crate itself,
        // then after the app crate is released, the kill handler will be dropped AFTER the app crate has been freed.
        // When it tries to drop the task's kill handler, a page fault will occur because the text section of the app crate has been unmapped.
        if let Some(kill_handler) = self.inner.lock().kill_handler.take() {
            warn!("While dropping task {:?}, its kill handler callback was still present. Removing it now.", self);
            drop(kill_handler);
        }
    }
}


/// A type wrapper that exposes public access to all inner fields of a task.
///
/// This is intended for use by the `task` crate, specifically within a `TaskRef`.
/// This can only be obtained by consuming a fully-initialized [`Task`],
/// which makes it completely safe to be public because there is nowhere else
/// besides within the `TaskRef::create()` constructor that one can obtain access
/// to an owned `Task` value that is already registered/spawned (actually usable).
///
/// If another crate instantiates a bare `Task` (not a `Taskref`) and then converts
/// it into this `ExposedTask` type, then there's nothing they can do with that task
/// because it cannot become a spawnable/schedulable/runnable task until it is
/// passed into `TaskRef::create()`, so that'd be completely harmless.
#[doc(hidden)]
pub struct ExposedTask {
    pub task: Task,
}
impl From<Task> for ExposedTask {
    fn from(task: Task) -> Self {
        Self { task }
    }
}
impl Deref for ExposedTask {
    type Target = Task;
    fn deref(&self) -> &Self::Target {
        &self.task
    }
}
// Here we simply expose accessors for all private fields of `Task`.
impl ExposedTask {
    #[inline(always)]
    pub fn inner(&self) -> &IrqSafeMutex<TaskInner> {
        &self.inner
    }
    #[inline(always)]
    pub fn tls_area(&self) -> &TlsDataImage {
        &self.tls_area
    }
    #[inline(always)]
    pub fn running_on_cpu(&self) -> &AtomicCell<OptionalCpuId> {
        &self.running_on_cpu
    }
    #[inline(always)]
    pub fn runstate(&self) -> &AtomicCell<RunState> {
        &self.runstate
    }
}


/// The states used to initialize a new `Task` when creating it; see [`Task::new()`].
///
/// Currently, this includes the states given in the [`InheritedStates::Custom`] variant.
pub enum InheritedStates<'t> {
    /// The new `Task` will inherit its states from the enclosed `Task`.
    FromTask(&'t Task),
    /// The new `Task` will be initialized with the enclosed custom states.
    Custom {
        mmi: MmiRef,
        namespace: Arc<CrateNamespace>,
        env: Arc<Mutex<Environment>>,
        app_crate: Option<Arc<AppCrateRef>>,
    }
}
impl<'t> From<&'t Task> for InheritedStates<'t> {
    fn from(task: &'t Task) -> Self {
        Self::FromTask(task)
    }
}
impl<'t> InheritedStates<'t> {
    fn into_tuple(self) -> (
        MmiRef,
        Arc<CrateNamespace>,
        Arc<Mutex<Environment>>,
        Option<Arc<AppCrateRef>>,
    ) {
        match self {
            Self::FromTask(task) => (
                task.mmi.clone(),
                task.namespace.clone(),
                task.inner.lock().env.clone(),
                task.app_crate.clone(),
            ),
            Self::Custom { mmi, namespace, env, app_crate } => (
                mmi,
                namespace,
                env,
                app_crate,
            )
        }
    }
}
