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
extern crate environment;
extern crate root;
extern crate x86_64;
extern crate spin;
extern crate kernel_config;
extern crate crossbeam_utils;


use core::fmt;
use core::hash::{Hash, Hasher};
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
use crossbeam_utils::atomic::AtomicCell;
use irq_safety::{MutexIrqSafe, interrupts_enabled, hold_interrupts};
use memory::MmiRef;
use stack::Stack;
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use mod_mgmt::{AppCrateRef, CrateNamespace, TlsDataImage};
use environment::Environment;
use spin::Mutex;
use x86_64::registers::model_specific::{GsBase, FsBase};


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
            .unwrap_or_else(|| String::new());
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
        if self.runstate() == RunState::Exited {
            self.runstate.store(RunState::Reaped);
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

    /// Blocks this `Task` by setting its runstate to [`RunState::Blocked`].
    pub fn block(&self) {
        self.runstate.store(RunState::Blocked);
    }

    /// Unblocks this `Task` by setting its runstate to [`RunState::Runnable`].
    pub fn unblock(&self) {
        self.runstate.store(RunState::Runnable);
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
    /// # Locking / Deadlock
    /// Obtains the locks on both this `Task`'s inner state and the given `next` `Task`'s inner state
    /// in order to mutate them. 
    #[doc(hidden)]
    pub fn task_switch(&self, next: &Task, apic_id: u8) {
        // debug!("task_switch [0]: (AP {}) prev {:?}, next {:?}, interrupts?: {}", apic_id, self, next, irq_safety::interrupts_enabled());

        // These conditions are checked elsewhere, but can be re-enabled if we want to be extra strict.
        // if !next.is_runnable() {
        //     error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was not Runnable! Current: {:?}, Next: {:?}", self, next);
        //     return;
        // }
        // if next.is_running() {
        //     error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was already running on AP {}!\nCurrent: {:?} Next: {:?}", apic_id, self, next);
        //     return;
        // }
        // if let Some(pc) = next.pinned_core() {
        //     if pc != apic_id {
        //         error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was pinned to AP {:?} but scheduled on AP {}!\n\tCurrent: {:?}, Next: {:?}", pc, apic_id, self, next);
        //         return;
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
        //         return;
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

        /// A private macro that actually calls the given context switch routine.
        macro_rules! call_context_switch {
            ($func:expr) => ( unsafe {
                $func(prev_task_saved_sp, next_task_saved_sp);
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
        // We cannot make any assumptions about what `next` is now, since it's unknown. 
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

        self.post_context_switch_action();
    }


    /// Perform any actions needed after a context switch.
    /// 
    /// Currently this only does two things:
    /// 1. Drops any data that the original previous task (before the context switch)
    ///    prepared for us to drop, as specified by `TaskInner::drop_after_task_switch`.
    /// 2. Obtains the preemption guard such that preemption can be re-enabled
    ///    when it is appropriate to do so.
    #[doc(hidden)]
    pub fn post_context_switch_action(&self) {
        // Step 1: 
        {
            let mut inner = self.inner.lock();
            let prev_task_data_to_drop = inner.drop_after_task_switch.take();
            drop(inner); // release the lock as soon as possible
            drop(prev_task_data_to_drop);
        }

        // Step 2: TODO
        // TODO
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        #[cfg(not(any(rq_eval, downtime_eval)))]
        trace!("Task::drop(): {}", self);

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


/// Represents a joinable [`TaskRef`], created by [`TaskRef::new()`].
/// Auto-derefs into a [`TaskRef`].
///
/// This allows another task to:
/// * [`join`] this task, i.e., wait for this task to finish executing,
/// * to obtain its [exit value] after it has completed.
/// 
/// ## [`Drop`]-based Behavior
/// The contained [`Task`] is joinable until this object is dropped.
/// When dropped, this task will be marked as non-joinable and treated as an "orphan" task.
/// This means that there is no way for another task to wait for it to complete
/// or obtain its exit value.
/// As such, this task will be auto-reaped after it exits (in order to avoid zombie tasks).
/// 
/// ## Not `Clone`-able
/// Due to the above drop-based behavior, this type must not implement `Clone`
/// because it assumes there is only ever one `JoinableTaskRef` per task.
/// 
/// However, this type auto-derefs into an inner [`TaskRef`], which *can* be cloned.
/// 
// /// Note: this type is considered an internal implementation detail.
// /// Instead, use the `TaskJoiner` type from the `spawn` crate, 
// /// which is intended to be the public-facing interface for joining a task.
#[derive(Debug)]
pub struct JoinableTaskRef {
    task: TaskRef,
}
assert_not_impl_any!(JoinableTaskRef: Clone);
impl Deref for JoinableTaskRef {
    type Target = TaskRef;
    fn deref(&self) -> &Self::Target {
        &self.task
    }
}
impl Drop for JoinableTaskRef {
    /// Marks the inner [`Task`] as not joinable, meaning that it is an orphaned task
    /// that will be auto-reaped after exiting.
    fn drop(&mut self) {
        self.task.joinable.store(false, Ordering::Relaxed);
    }
}


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
#[derive(Debug, Clone)]
pub struct TaskRef(Arc<Task>);

impl TaskRef {
    /// Creates a new `TaskRef`, a shareable wrapper around the given `Task`.
    /// 
    /// This function also initializes the given `Task`'s `TaskLocalData` struct,
    /// which will be used to determine the current `Task` on each CPU.
    /// 
    /// It does *not* add this task to the system-wide task list or any runqueues,
    /// nor does it schedule this task in.
    /// 
    /// ## Return
    /// Returns a [`JoinableTaskRef`], which derefs into the newly-created `TaskRef`
    /// and can be used to "join" this task (wait for it to exit) and obtain its exit value.
    pub fn new(task: Task) -> JoinableTaskRef {
        let task_id = task.id;
        let taskref = TaskRef(Arc::new(task));
        let tld = TaskLocalData {
            taskref: taskref.clone(),
            task_id,
        };
        taskref.0.inner.lock().task_local_data = Some(Box::new(tld));

        // Mark this task as joinable, now that it has been wrapped in the proper type.
        taskref.joinable.store(true, Ordering::Relaxed);
        JoinableTaskRef { task: taskref }
    }

    /// Blocks until this task has exited or has been killed.
    ///
    /// Returns `Ok()` once this task has exited,
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
        while !self.0.has_exited() { }

        // Then, wait for it to actually stop running on any CPU core.
        while self.0.is_running() { }

        Ok(())
    }

    /// Call this function to indicate that this task has successfully ran to completion,
    /// and that it has returned the given `exit_value`.
    /// 
    /// This should only be used within task cleanup functions to indicate
    /// that the current task has cleanly exited.
    /// 
    /// # Locking / Deadlock
    /// This method obtains a writable lock on the underlying Task's inner state.
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
    /// This method obtains a writable lock on the underlying Task's inner state.
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
        let curr_task = get_my_current_task().ok_or("mark_as_exited(): failed to check the current task")?;
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
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on this `Task`'s inner state in order to mutate it. 
    fn internal_exit(&self, val: ExitValue) -> Result<(), &'static str> {
        if self.0.has_exited() {
            return Err("BUG: task was already exited! (did not overwrite its existing exit value)");
        }
        {
            let mut inner = self.0.inner.lock();
            inner.exit_value = Some(val);
            self.0.runstate.store(RunState::Exited);

            // Corner case: if the task isn't currently running (as with killed tasks), 
            // we must clean it up now rather than in `task_switch()`, as it will never be scheduled in again.
            if !self.0.is_running() {
                trace!("internal_exit(): dropping TaskLocalData for non-running task {}", &*self.0);
                drop(inner.task_local_data.take());
            }
        }

        #[cfg(runqueue_spillful)] {   
            if let Some(remove_from_runqueue) = RUNQUEUE_REMOVAL_FUNCTION.get() {
                if let Some(rq) = self.on_runqueue() {
                    remove_from_runqueue(self, rq)?;
                }
            }
        }

        Ok(())
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
        self.0.deref()
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
) -> Result<JoinableTaskRef, &'static str> {
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
    bootstrap_task.runstate.store(RunState::Runnable);
    bootstrap_task.running_on_cpu.store(Some(apic_id).into()); 
    bootstrap_task.inner.get_mut().pinned_core = Some(apic_id); // can only run on this CPU core
    let bootstrap_task_id = bootstrap_task.id;
    let task_ref = TaskRef::new(bootstrap_task);

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
