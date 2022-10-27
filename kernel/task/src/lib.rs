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
#![feature(const_btree_new)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate static_assertions;
extern crate irq_safety;
extern crate memory;
extern crate stack;
extern crate tss;
extern crate mod_mgmt;
extern crate context_switch;
extern crate preemption;
extern crate environment;
extern crate root;
extern crate x86_64;
extern crate spin;
extern crate kernel_config;
extern crate crossbeam_utils;


use core::{
    any::Any,
    fmt,
    hash::{Hash, Hasher},
    ops::Deref,
    panic::PanicInfo,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::String,
    sync::Arc,
};
use crossbeam_utils::atomic::AtomicCell;
use irq_safety::{MutexIrqSafe, interrupts_enabled, hold_interrupts};
use memory::MmiRef;
use stack::Stack;
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use mod_mgmt::{AppCrateRef, CrateNamespace, TlsDataImage};
use environment::Environment;
use spin::Mutex;
use x86_64::registers::model_specific::{GsBase, FsBase};
use preemption::PreemptionGuard;


/// The function signature of the callback that will be invoked
/// when a given Task panics or otherwise fails, e.g., a machine exception occurs.
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
            .map(|m| format!("{}", m))
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


/// The list of all Tasks in the system.
pub static TASKLIST: MutexIrqSafe<BTreeMap<usize, TaskRef>> = MutexIrqSafe::new(BTreeMap::new());


/// returns a shared reference to the `Task` specified by the given `task_id`
pub fn get_task(task_id: usize) -> Option<TaskRef> {
    TASKLIST.lock().get(&task_id).cloned()
}


/// Registers a kill handler function for the current `Task`.
/// 
/// [`KillHandler`]s are called when a `Task` panics or otherwise fails
/// (e.g., due to a machine exception).
///
/// # Locking / Deadlock
/// Obtains the lock on this `Task`'s inner state in order to mutate it.
pub fn set_kill_handler(function: KillHandler) -> Result<(), &'static str> {
    get_my_current_task()
        .ok_or("couldn't get current task")
        .map(|t| t.inner.lock().kill_handler = Some(function))
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
    get_my_current_task()
        .and_then(|t| t.inner.lock().kill_handler.take())
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
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
    Exited,
    /// This `Task` had already exited and its `ExitValue` has been taken;
    /// its exit value can only be taken once, and consumed by another `Task`.
    /// This `Task` is now useless, and can be deleted and removed from the Task list.
    Reaped,
}


#[cfg(runqueue_spillful)]
/// A callback that will be invoked to remove a specific task from a specific runqueue.
/// Should be initialized by the runqueue crate.
pub static RUNQUEUE_REMOVAL_FUNCTION: spin::Once<fn(&TaskRef, u8) -> Result<(), &'static str>> = spin::Once::new();


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

/// The signature of a Task's failure cleanup function.
pub type FailureCleanupFunction = fn(TaskRef, KillReason) -> !;


/// A wrapper around `Option<u8>` with a forced type alignment of 2 bytes,
/// which guarantees that it compiles down to lock-free native atomic instructions
/// when using it inside of an atomic type like [`AtomicCell`].
#[derive(Copy, Clone)]
#[repr(align(2))]
struct OptionU8(Option<u8>);
impl From<Option<u8>> for OptionU8 {
    fn from(opt: Option<u8>) -> Self {
        OptionU8(opt)
    }
}
impl Into<Option<u8>> for OptionU8 {
    fn into(self) -> Option<u8> {
        self.0
    }
}
impl fmt::Debug for OptionU8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
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
/// primarily by the `spawn` crate for creating and running new tasks. 
pub struct TaskInner {
    /// The status or value this Task exited with, if it has already exited.
    exit_value: Option<ExitValue>,
    /// the saved stack pointer value, used for task switching.
    pub saved_sp: usize,
    /// A reference to this task's `TaskLocalData` struct, which is used to quickly retrieve the "current" Task
    /// on a given CPU core. 
    /// The `TaskLocalData` refers back to this `Task` struct, thus it must be initialized later
    /// after the task has been fully created, which currently occurs in `TaskRef::new()`.
    task_local_data: Option<Box<TaskLocalData>>,
    /// The preemption guard that was used for safely task switching to this task.
    ///
    /// The `PreemptionGuard` is stored here right before a context switch begins
    /// and then retrieved from here right after the context switch ends.
    ///
    /// TODO: this (and perhaps `task_local_data`) should be kept in per-CPU variables
    ///       rather than within the `TaskInner` structure, because they aren't really related
    ///       to a specific task, but rather to a specific CPU's preemption status.
    preemption_guard: Option<PreemptionGuard>,
    /// Data that should be dropped after switching away from a task that has exited.
    /// Currently, this only contains the previous Task's [`TaskLocalData`].
    drop_after_task_switch: Option<Box<TaskLocalData>>,
    /// The kernel stack, which all `Task`s must have in order to execute.
    pub kstack: Stack,
    /// Whether or not this task is pinned to a certain core.
    /// The idle tasks are always pinned to their respective cores.
    pub pinned_core: Option<u8>,
    /// The function that will be called when this `Task` panics or fails due to a machine exception.
    /// It will be invoked before the task is cleaned up via stack unwinding.
    /// This is similar to Rust's built-in panic hook, but is also called upon a machine exception, not just a panic.
    kill_handler: Option<KillHandler>,
    /// The environment variables for this task, which are shared among child and parent tasks by default.
    env: Arc<Mutex<Environment>>,
    /// Stores the restartable information of the task. 
    /// `Some(RestartInfo)` indicates that the task is restartable.
    pub restart_info: Option<RestartInfo>,
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
    /// This is not public because it permits interior mutability.
    inner: MutexIrqSafe<TaskInner>,

    /// The unique identifier of this Task.
    pub id: usize,
    /// The simple name of this Task.
    pub name: String,
    /// Which cpu core this Task is currently running on;
    /// `None` if not currently running.
    /// We use `OptionU8` instead of `Option<u8>` to ensure that 
    /// this field is accessed using lock-free native atomic instructions.
    ///
    /// This is not public because it permits interior mutability.
    running_on_cpu: AtomicCell<OptionU8>,
    /// The runnability of this task, i.e., whether it's eligible to be scheduled in.
    ///
    /// This is not public because it permits interior mutability.
    runstate: AtomicCell<RunState>,
    /// Whether this Task is joinable.
    /// * If `true`, another task holds the [`JoinableTaskRef`] object that was created
    ///   by [`TaskRef::new()`], which indicates that that other task is able to
    ///   wait for this task to exit and thus be able to obtain this task's exit value.
    /// * If `false`, the [`JoinableTaskRef`] was dropped, and therefore no other task
    ///   can join this task or obtain its exit value.
    /// 
    /// This is not public because it permits interior mutability.
    joinable: AtomicBool,
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
    /// The function that should be run as a last-ditch attempt to recover from this task's failure,
    /// e.g., this can be called when unwinding itself fails. 
    /// Typically, it will point to this Task's specific instance of `spawn::task_cleanup_failure()`,
    /// which has generic type parameters that describe its function signature, argument type, and return type.
    pub failure_cleanup_function: FailureCleanupFunction,
    /// The Thread-Local Storage (TLS) area for this task.
    /// 
    /// Upon each task switch, we must set the value of the TLS base register 
    /// (e.g., FS_BASE on x86_64) to the value of this TLS area's self pointer.
    tls_area: TlsDataImage,
    
    #[cfg(runqueue_spillful)]
    /// The runqueue that this Task is on.
    on_runqueue: AtomicCell<OptionU8>,

    #[cfg(simd_personality)]
    /// Whether this Task is SIMD enabled and what level of SIMD extensions it uses.
    pub simd: SimdExt,
}

// Ensure that atomic fields in the `Tast` struct are actually lock-free atomics.
const_assert!(AtomicCell::<OptionU8>::is_lock_free());
const_assert!(AtomicCell::<RunState>::is_lock_free());

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut ds = f.debug_struct("Task");
        ds.field("name", &self.name)
            .field("id", &self.id)
            .field("running_on", &self.running_on_cpu())
            .field("runstate", &self.runstate());
        if let Some(inner) = self.inner.try_lock() {
            ds.field("pinned", &inner.pinned_core);
        } else {
            ds.field("pinned", &"<Locked>");
        }
        ds.finish()
    }
}

impl Hash for Task {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.id.hash(h);
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
        let (mmi, namespace, env, app_crate) = (
            Arc::clone(&curr_task.mmi),
            Arc::clone(&curr_task.namespace),
            Arc::clone(&curr_task.inner.lock().env),
            curr_task.app_crate.clone(),
        );

        let kstack = kstack
            .or_else(|| stack::alloc_stack(KERNEL_STACK_SIZE_IN_PAGES, &mut mmi.lock().page_table))
            .ok_or("couldn't allocate kernel stack!")?;

        Ok(Task::new_internal(kstack, mmi, namespace, env, app_crate, failure_cleanup_function))
    }
    
    /// The internal routine for creating a `Task`, which does not make assumptions 
    /// about whether a currently-running `Task` exists or whether the new `Task`
    /// should inherit any states from it.
    fn new_internal(
        kstack: Stack, 
        mmi: MmiRef,
        namespace: Arc<CrateNamespace>,
        env: Arc<Mutex<Environment>>,
        app_crate: Option<Arc<AppCrateRef>>,
        failure_cleanup_function: FailureCleanupFunction,
    ) -> Self {
         /// The counter of task IDs
        static TASKID_COUNTER: AtomicUsize = AtomicUsize::new(0);

        // we should re-use old task IDs again, instead of simply blindly counting up
        // TODO FIXME: or use random values to avoid state spill
        let task_id = TASKID_COUNTER.fetch_add(1, Ordering::Acquire);

        // Obtain a new copied instance of the TLS data image for this task.
        let tls_area = namespace.get_tls_initializer_data();

        Task {
            inner: MutexIrqSafe::new(TaskInner {
                exit_value: None,
                saved_sp: 0,
                task_local_data: None,
                preemption_guard: None,
                drop_after_task_switch: None,
                kstack,
                pinned_core: None,
                kill_handler: None,
                env,
                restart_info: None,
            }),
            id: task_id,
            name: format!("task_{}", task_id),
            running_on_cpu: AtomicCell::new(None.into()),
            runstate: AtomicCell::new(RunState::Initing),
            // Tasks are not considered "joinable" until passed to `TaskRef::new()`
            joinable: AtomicBool::new(false),
            mmi,
            is_an_idle_task: false,
            app_crate,
            namespace,
            failure_cleanup_function,
            tls_area,

            #[cfg(runqueue_spillful)]
            on_runqueue: AtomicCell::new(None.into()),
            
            #[cfg(simd_personality)]
            simd: SimdExt::None,
        }
    }

    /// Sets the `Environment` of this Task.
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on this `Task`'s inner state in order to mutate it.
    pub fn set_env(&self, new_env:Arc<Mutex<Environment>>) {
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

    /// Returns the APIC ID of the CPU this `Task` is currently running on.
    pub fn running_on_cpu(&self) -> Option<u8> {
        self.running_on_cpu.load().into()
    }

    /// Returns `true` if this task is joinable, `false` if not.
    /// 
    /// * If `true`, another task holds the [`JoinableTaskRef`] object that was created
    ///   by [`TaskRef::new()`], which indicates that that other task is able to
    ///   wait for this task to exit and thus be able to obtain this task's exit value.
    /// * If `false`, the `TaskJoiner` object was dropped, and therefore no other task
    ///   can join this task or obtain its exit value.
    /// 
    /// When a task is not joinable, it is considered to be an orphan
    /// and will thus be automatically reaped and cleaned up once it exits
    /// because no other task is waiting on it to exit.
    #[doc(alias("orphan", "zombie"))]
    pub fn is_joinable(&self) -> bool {
        self.joinable.load(Ordering::Relaxed)
    }

    /// Returns the APIC ID of the CPU this `Task` is pinned on,
    /// or `None` if it is not pinned.
    pub fn pinned_core(&self) -> Option<u8> {
        self.inner.lock().pinned_core.clone()
    }

    /// Returns the current [`RunState`] of this `Task`.
    pub fn runstate(&self) -> RunState {
        self.runstate.load()
    }

    /// Returns `true` if this `Task` is Runnable, i.e., able to be scheduled in.
    ///
    /// # Note
    /// This does *NOT* mean that this `Task` is actually currently running, just that it is *able* to be run.
    pub fn is_runnable(&self) -> bool {
        self.runstate() == RunState::Runnable
    }

    /// Returns the namespace in which this `Task` is loaded/linked into and runs within.
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
    /// before you enclose it in a `TaskRef` wrapper type.
    ///
    /// # Locking / Deadlock
    /// Because this function requires a mutable reference to this `Task`,
    /// no locks must be obtained. 
    pub fn inner_mut(&mut self) -> &mut TaskInner {
        self.inner.get_mut()
    }

    /// Exposes read-only access to this `Task`'s [`RestartInfo`] by invoking
    /// the given `func` with a reference to its `RestartInfo`.
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
    /// if its RunState is either `Exited` or `Reaped`.
    pub fn has_exited(&self) -> bool {
        match self.runstate() {
            RunState::Exited | RunState::Reaped => true,
            _ => false,
        }
    }

    /// Returns `true` if this is an application `Task`. 
    /// This will also return `true` if this task was spawned by an application task,
    /// since a task inherits the "application crate" field from its "parent" who spawned it.
    pub fn is_application(&self) -> bool {
        self.app_crate.is_some()
    }

    /// Returns `true` if this is a userspace `Task`.
    /// Currently userspace support is disabled, so this always returns `false`.
    pub fn is_userspace(&self) -> bool {
        // self.ustack.is_some()
        false
    }

    /// Returns `true` if this `Task` was spawned as a restartable task.
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on this `Task`'s inner state in order to access it.
    pub fn is_restartable(&self) -> bool {
        self.inner.lock().restart_info.is_some()
    }

    /// Takes ownership of this `Task`'s exit value and returns it,
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
    /// Obtains the lock on this `Task`'s inner state in order to mutate it.    
    #[doc(alias("reap"))]
    pub fn take_exit_value(&self) -> Option<ExitValue> {
        if self.runstate.compare_exchange(RunState::Exited, RunState::Reaped).is_ok() {
            TASKLIST.lock().remove(&self.id);
            self.inner.lock().exit_value.take()
        } else {
            None
        }
    }

    #[cfg(runqueue_spillful)]
    /// Returns the runqueue on which this `Task` is currently enqueued.
    pub fn on_runqueue(&self) -> Option<u8> {
        self.on_runqueue.load().into()
    }

    #[cfg(runqueue_spillful)]
    /// Marks this `Task` as enqueued on the given runqueue.
    pub fn set_on_runqueue(&self, runqueue: Option<u8>) {
        self.on_runqueue.store(runqueue.into());
    }

    pub fn init(self) -> TaskRef<true, true> {
        // FIXME: document
        assert!(self.runstate.compare_exchange(RunState::Initing, RunState::Blocked).is_ok());

        let task_id = self.id;
        let task_ref = TaskRef {
            task: Arc::new(self),
        };
        let tld = TaskLocalData {
            // clone from <true, true> to <false, true>
            // transmute from <false, true> to <false, false>
            taskref: unsafe { core::mem::transmute(task_ref.clone()) },
            task_id,
        };
        task_ref.inner.lock().task_local_data = Some(Box::new(tld));

        // Mark this task as joinable, now that it has been wrapped in the proper type.
        task_ref.joinable.store(true, Ordering::Relaxed);
        task_ref
    }

    /// Sets this `Task` as this core's current task.
    /// 
    /// Currently this is achieved by writing a pointer to the `TaskLocalData` 
    /// into the `GS_BASE` register.
    /// This also updates the current TLS region, which is stored in `FS_BASE`.
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on this `Task`'s inner state in order to access it. 
    fn set_as_current_task(&self) {
        FsBase::write(x86_64::VirtAddr::new(self.tls_area.pointer_value() as u64));

        // TODO: now that proper ELF TLS areas are supported, 
        //       use that TLS area for the `TaskLocalData` instead of `GS_BASE`. 

        if let Some(ref tld) = self.inner.lock().task_local_data {
            GsBase::write(x86_64::VirtAddr::new(tld.deref() as *const _ as u64));
        } else {
            error!("BUG: failed to set current task, it had no TaskLocalData. {:?}", self);
        }
    }

    /// Switches from the current task (`self`) to the given `next` task.
    /// 
    /// ## Arguments
    /// * `next`: the task to switch to.
    /// * `apic_id`: the ID of the current CPU.
    /// * `preemption_guard`: a guard that is used to ensure preemption is disabled
    ///   for the duration of this task switch operation.
    ///
    /// ## Important Note about Control Flow
    /// If this is the first time that `next` task has been switched to,
    /// the control flow will *NOT* return from this function,
    /// and will instead jump to a wrapper function that will directly invoke
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
    ///    * If `false`, the task switch did not occur, and `self` is still the current task.
    /// 2. a [`PreemptionGuard`] that allows the caller to determine for how long
    ///    preemption remains disabled, i.e., until the guard is dropped.
    ///
    /// ## Locking / Deadlock
    /// Obtains brief locks on both this `Task`'s inner state and
    /// the given `next` `Task`'s inner state in order to mutate them.
    pub fn task_switch<const JOINABLE: bool, const UNBLOCKABLE: bool>(
        &self,
        next: TaskRef<JOINABLE, UNBLOCKABLE>,
        apic_id: u8,
        preemption_guard: PreemptionGuard,
    ) -> (bool, PreemptionGuard) {
        // No need to task switch if the next task is the same as the current task.
        if self.id == next.id {
            return (false, preemption_guard);
        }

        // trace!("task_switch [0]: (CPU {}) prev {:?}, next {:?}, interrupts?: {}", apic_id, self, next, irq_safety::interrupts_enabled());

        // These conditions are checked elsewhere, but can be re-enabled if we want to be extra strict.
        // if !next.is_runnable() {
        //     error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was not Runnable! Current: {:?}, Next: {:?}", self, next);
        //     return (false, preemption_guard);
        // }
        // if next.is_running() {
        //     error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was already running on AP {}!\nCurrent: {:?} Next: {:?}", apic_id, self, next);
        //     return (false, preemption_guard);
        // }
        // if let Some(pc) = next.pinned_core() {
        //     if pc != apic_id {
        //         error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was pinned to AP {:?} but scheduled on AP {}!\n\tCurrent: {:?}, Next: {:?}", pc, apic_id, self, next);
        //         return (false, preemption_guard);
        //     }
        // }

        // Note that because userspace support is currently disabled, this will never happen.
        // // Change the privilege stack (RSP0) in the TSS.
        // // We can safely skip setting the TSS RSP0 when switching to a kernel task, 
        // // i.e., when `next` is not a userspace task.
        // if next.is_userspace() {
        //     let (stack_bottom, stack_size) = {
        //         let kstack = &next.inner.lock().kstack;
        //         (kstack.bottom(), kstack.size_in_bytes())
        //     };
        //     let new_tss_rsp0 = stack_bottom + (stack_size / 2); // the middle half of the stack
        //     if tss::tss_set_rsp0(new_tss_rsp0).is_ok() { 
        //         // debug!("task_switch [2]: new_tss_rsp = {:#X}", new_tss_rsp0);
        //     } else {
        //         error!("task_switch(): failed to set AP {} TSS RSP0, aborting task switch!", apic_id);
        //         return (false, preemption_guard);
        //     }
        // }

        // // Switch page tables. 
        // // Since there is only a single address space (as userspace support is currently disabled),
        // // we do not need to do this at all.
        // if false {
        //     let prev_mmi = &self.mmi;
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
        //         //         self.name, prev_mmi_locked.page_table, next.name, next_mmi_locked.page_table);
        //
        //         prev_mmi_locked.page_table.switch(&next_mmi_locked.page_table);
        //     }
        // }
       
        // Set runstates and current task *atomically*, i.e., by disabling interrupts.
        // This is necessary to ensure that any interrupt handlers that may run on this CPU
        // during the schedule/task_switch routines cannot observe inconsistencies
        // in task runstates, e.g., when an interrupt handler accesses the current task context.
        {
            let _held_interrupts = hold_interrupts();
            self.running_on_cpu.store(None.into()); // no longer running
            next.running_on_cpu.store(Some(apic_id).into()); // now running on this core
            next.set_as_current_task();
            drop(_held_interrupts);
        }

        // Move the preemption guard into the next task such that we can use retrieve it
        // after the below context switch operation has completed.
        //
        // TODO: this should be moved into per-CPU storage areas rather than the task struct.
        {
            next.inner.lock().preemption_guard = Some(preemption_guard);
        }

        // If the current task is exited, then we need to remove the cyclical TaskRef reference in its TaskLocalData.
        // We store the removed TaskLocalData in the next Task struct so that we can access it after the context switch.
        if self.has_exited() {
            // trace!("task_switch(): preparing to drop TaskLocalData for running task {}", self);
            next.inner.lock().drop_after_task_switch = self.inner.lock().task_local_data.take();
        }

        let prev_task_saved_sp: *mut usize = {
            let mut inner = self.inner.lock(); // ensure the lock is released
            (&mut inner.saved_sp) as *mut usize
        };
        let next_task_saved_sp: usize = {
            let inner = next.inner.lock(); // ensure the lock is released
            inner.saved_sp
        };
        // debug!("task_switch [4]: prev sp: {:#X}, next sp: {:#X}", prev_task_saved_sp as usize, next_task_saved_sp);

        /// A macro that drops the `next` TaskRef and then calls the given context switch routine.
        macro_rules! call_context_switch {
            ($func:expr) => ({
                drop(next);
                unsafe {
                    $func(prev_task_saved_sp, next_task_saved_sp);
                }
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
        ///////////////////////////////////////////////////////////////////////////////////////////
        // *** Important Notes about Behavior after a Context Switch ***
        //
        // Here, after the actual context switch operation above,
        // `self` (curr) is now `next` because the stacks have been switched, 
        // and `next` has become another random task based on a previous task switch.
        // 
        // We cannot make any assumptions about what `next` is now, since it's unknown.
        // Hence why we dropped the local `TaskRef` for `next` right before the context switch.
        //
        // If this is **NOT** the first time the newly-current task (`self`) has run,
        // then it will resume execution below as normal because this is where it left off
        // when the context switch operation occurred.
        //
        // However, if this **is** the first time that the newly-current task (`self`) 
        // has been switched to and is running, the control flow will **NOT** proceed here.
        // Instead, it will have directly jumped to its entry point, which is `task_wrapper()`.
        //
        // As such, anything we do below should also be done in `task_wrapper()`.
        // Thus, we want to ensure that post-context switch actions below are kept minimal
        // and are easy to replicate in `task_wrapper()`.
        ///////////////////////////////////////////////////////////////////////////////////////////

        let recovered_preemption_guard = self.post_context_switch_action();
        (true, recovered_preemption_guard)
    }


    /// Perform any actions needed after a context switch.
    /// 
    /// Currently this only does two things:
    /// 1. Drops any data that the original previous task (before the context switch)
    ///    prepared for us to drop, as specified by `TaskInner::drop_after_task_switch`.
    /// 2. Obtains the preemption guard such that preemption can be re-enabled
    ///    when it is appropriate to do so.
    #[doc(hidden)]
    pub fn post_context_switch_action(&self) -> PreemptionGuard {
        // Step 1: drop data from previously running task
        {
            let prev_task_data_to_drop = self.inner.lock().drop_after_task_switch.take();
            drop(prev_task_data_to_drop);
        }

        // Step 2: retake ownership of preemption guard in order to re-enable preemption.
        {
            self.inner
                .lock()
                .preemption_guard
                .take()
                .expect("BUG: post_context_switch_action: no PreemptionGuard existed")
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        #[cfg(not(any(rq_eval, downtime_eval)))]
        trace!("[CPU {}] Task::drop(): {}", apic::get_my_apic_id(), self);

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

impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{{{}}}", self.name, self.id)
    }
}



// impl Drop for TaskRef {
//     fn drop(&mut self) {
//         trace!("Dropping TaskRef: strong_refs: {}, {:?}", Arc::strong_count(&self.0), self);
//     }
// }


/// Bootstrap a new task from the current thread of execution.
/// 
/// # Note
/// This function does not add the new task to any runqueue.
pub fn bootstrap_task(
    apic_id: u8, 
    stack: Stack,
    kernel_mmi_ref: MmiRef,
) -> Result<TaskRef<true, false>, &'static str> {
    // Here, we cannot call `Task::new()` because tasking hasn't yet been set up for this core.
    // Instead, we generate all of the `Task` states manually, and create an initial task directly.
    let default_namespace = mod_mgmt::get_initial_kernel_namespace()
        .ok_or("The initial kernel CrateNamespace must be initialized before the tasking subsystem.")?
        .clone();
    let default_env = Arc::new(Mutex::new(Environment::default()));
    let mut bootstrap_task = Task::new_internal(
        stack,
        kernel_mmi_ref,
        default_namespace,
        default_env,
        None,
        bootstrap_task_cleanup_failure,
    );
    bootstrap_task.name = format!("bootstrap_task_core_{}", apic_id);
    bootstrap_task.running_on_cpu.store(Some(apic_id).into()); 
    bootstrap_task.inner.get_mut().pinned_core = Some(apic_id); // can only run on this CPU core
    let bootstrap_task_id = bootstrap_task.id;
    let (task_ref, _) = bootstrap_task.init().unblock();

    // set this as this core's current task, since it's obviously running
    task_ref.set_as_current_task();
    if get_my_current_task().is_none() {
        error!("BUG: bootstrap_task(): failed to properly set the new boostrapped task as the current task on AP {}", apic_id);
        return Err("BUG: bootstrap_task(): failed to properly set the new bootstrapped task as the current task");
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
    error!("BUG: bootstrap_task_cleanup_failure: {:?} died with {:?}\n. There's nothing we can do here; looping indefinitely!", current_task, kill_reason);
    loop { }
}


/// The structure that holds information local to each Task,
/// effectively a form of thread-local storage (TLS).
/// A pointer to this structure is stored in the `GS_BASE` MSR (model-specific register),
/// such that any task can easily and quickly access their local data.
/// 
/// TODO: combine this into the standard TLS area, which uses FS_BASE plus the FS segment register.
// #[repr(C)]
#[derive(Debug)]
struct TaskLocalData {
    taskref: TaskRef,
    task_id: usize,
}

/// Returns a reference to the current task's `TaskLocalData` 
/// by using the `TaskLocalData` pointer stored in the `GS_BASE` register.
fn get_task_local_data() -> Option<&'static TaskLocalData> {
    let tld: &'static TaskLocalData = {
        let tld_ptr = GsBase::read().as_u64() as *const TaskLocalData;
        if tld_ptr.is_null() {
            return None;
        }
        // SAFE: it's safe to cast this as a static reference
        // because it will always be valid for the life of a given Task's execution.
        unsafe { &*tld_ptr }
    };
    Some(tld)
}

/// Returns a reference to the current task.
pub fn get_my_current_task() -> Option<&'static TaskRef> {
    get_task_local_data().map(|tld| &tld.taskref)
}

/// Returns the current task's ID.
pub fn get_my_current_task_id() -> Option<usize> {
    get_task_local_data().map(|tld| tld.task_id)
}

pub mod rref;
pub use rref::*;
