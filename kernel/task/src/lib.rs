//! This crate contains the `Task` structure for supporting multithreading, 
//! and the associated code for dealing with tasks.
//! 
//! To create new `Task`s, use the [`spawn`](../spawn/index.html) crate.
//! 
//! # Examples
//! How to wait for a `Task` to finish (using `join()`) and get its exit value.
//! ```
//! // `taskref` is the task that we're waiting on
//! if let Ok(exit_value) = taskref.join() {
//!     match exit_value {
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

// TODO: Add direct explanation to why this empty loop is necessary and criteria for replacing it with something else
#![allow(clippy::empty_loop)]
#![no_std]
#![feature(panic_info_message)]
#![feature(thread_local)]
#![feature(negative_impls)]

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
extern crate no_drop;


use core::{
    any::Any,
    fmt,
    hash::{Hash, Hasher},
    ops::Deref,
    panic::PanicInfo,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering, fence},
    task::Waker,
};
use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::String,
    sync::Arc,
};
use crossbeam_utils::atomic::AtomicCell;
use irq_safety::{MutexIrqSafe, hold_interrupts};
use memory::MmiRef;
use stack::Stack;
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use mod_mgmt::{AppCrateRef, CrateNamespace, TlsDataImage};
use environment::Environment;
use spin::Mutex;
use x86_64::registers::model_specific::FsBase;
use preemption::PreemptionGuard;
use no_drop::NoDrop;

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
    with_current_task(|t| {
        t.inner.lock().kill_handler = Some(function);
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
    with_current_task(|t| t.inner.lock().kill_handler.take())
        .ok()
        .flatten()
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
pub type FailureCleanupFunction = fn(ExitableTaskRef, KillReason) -> !;


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
impl From<OptionU8> for Option<u8> {
    fn from(val: OptionU8) -> Self {
        val.0
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
    /// the saved stack pointer value, used for task switching.
    pub saved_sp: usize,
    /// The preemption guard that was used for safely task switching to this task.
    ///
    /// The `PreemptionGuard` is stored here right before a context switch begins
    /// and then retrieved from here right after the context switch ends.
    ///
    /// TODO: this should be kept in a per-CPU variable rather than within
    ///       the `TaskInner` structure, because it's not really related
    ///       to a specific task, but rather to a specific CPU's preemption status.
    preemption_guard: Option<PreemptionGuard>,
    /// Data that should be dropped after switching away from a task that has exited.
    /// Currently, this contains the previous Task's `TaskRef` removed from its TLS area.
    drop_after_task_switch: Option<TaskRef>,
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
    /// The waker that is awoken when this task completes.
    waker: Option<Waker>,
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
    /// Whether the task is suspended.
    ///
    /// This is only triggered by a Ctrl + Z in the terminal.
    ///
    /// This is not public because it permits interior mutability.
    suspended: AtomicBool,
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
    
    #[cfg(simd_personality)]
    /// Whether this Task is SIMD enabled and what level of SIMD extensions it uses.
    pub simd: SimdExt,
}

// Ensure that atomic fields in the `Tast` struct are actually lock-free atomics.
const _: () = assert!(AtomicCell::<OptionU8>::is_lock_free());
const _: () = assert!(AtomicCell::<RunState>::is_lock_free());

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
    /// 
    /// By default, the new `Task` will inherit some of its states from the given `parent_task`:
    /// its `Environment`, `MemoryManagementInfo`, `CrateNamespace`, and `app_crate` reference.
    /// If necessary, those states can be changed by setting them for the returned `Task`.
    /// 
    /// # Arguments
    /// * `kstack`: the optional kernel `Stack` for this `Task` to use.
    ///    * If `None`, a kernel stack of the default size will be allocated and used.
    /// * `parent_task`: the optional `TaskRef` that acts as a sort of "parent" template
    ///    for this new `Task`.
    ///    Theseus doesn't have a true parent-child relationship between tasks;
    ///    the new `Task` merely inherits certain states from this `parent_task`.
    ///    * If `None`, the current task is used to determine the initial values of those states.
    ///      This means that the tasking infrastructure must have been initialized before
    ///      this function can be invoked with a `parent_task` value of `None`.
    /// * `failure_cleanup_function`: an error handling function that acts as a last resort
    ///    when all else fails, e.g., if unwinding crashes.
    /// 
    /// ## Note
    /// * If invoking this function with a `parent_task` value of `None`,
    ///   tasking must have already been initialized so the current task can be obtained.
    /// * This does not run the task, schedule it in, or switch to it.
    /// * If you want to create a new task, you should use the `spawn` crate instead.
    pub fn new(
        kstack: Option<Stack>,
        parent_task: Option<&TaskRef>,
        failure_cleanup_function: FailureCleanupFunction,
    ) -> Result<Task, &'static str> {
        let clone_inherited_items = |taskref: &TaskRef| {
            (
                taskref.mmi.clone(),
                taskref.namespace.clone(),
                taskref.inner.lock().env.clone(),
                taskref.app_crate.clone(),
            )
        };
        let (mmi, namespace, env, app_crate) = parent_task
            .map(clone_inherited_items)
            .ok_or(())
            .or_else(|_| with_current_task(clone_inherited_items))
            .map_err(|_| "Task::new(): `parent_task` wasn't provided, and couldn't get current task")?;

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
        let task_id = TASKID_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Obtain a new copied instance of the TLS data image for this task.
        let tls_area = namespace.get_tls_initializer_data();

        Task {
            inner: MutexIrqSafe::new(TaskInner {
                saved_sp: 0,
                preemption_guard: None,
                drop_after_task_switch: None,
                kstack,
                pinned_core: None,
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
            // Tasks are not considered "joinable" until passed to `TaskRef::new()`
            joinable: AtomicBool::new(false),
            mmi,
            is_an_idle_task: false,
            app_crate,
            namespace,
            failure_cleanup_function,
            tls_area,

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
    /// * If `false`, the [`JoinableTaskRef`] object was dropped, and therefore no other task
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
        self.inner.lock().pinned_core
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
        matches!(self.runstate(), RunState::Exited | RunState::Reaped)
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

    /// Blocks this `Task` by setting its runstate to [`RunState::Blocked`].
    ///
    /// Returns the previous runstate on success, and the current runstate on
    /// error. Will only suceed if the task is runnable or already blocked.
    pub fn block(&self) -> Result<RunState, RunState> {
        use RunState::{Blocked, Runnable};

        if self.runstate.compare_exchange(Runnable, Blocked).is_ok() {
            Ok(Runnable)
        } else if self.runstate.compare_exchange(Blocked, Blocked).is_ok() {
            warn!("Blocked an already blocked task: {:?}\n\t --> Current {:?}",
                self, get_my_current_task()
            );
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
            warn!("Unblocked an already runnable task: {:?}\n\t --> Current {:?}",
                self, get_my_current_task()
            );
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
    
    /// Sets the waker to be awoken when this task exits.
    pub fn set_waker(&self, waker: Waker) {
        self.inner.lock().waker = Some(waker);
    }

    /// Sets this `Task` as this CPU's current task.
    /// 
    /// This updates the current TLS area, which is written to the `FS_BASE` MSR on x86_64.
    fn set_as_current_task(&self) {
        FsBase::write(x86_64::VirtAddr::new(self.tls_area.pointer_value() as u64));
    }

    /// Perform any actions needed after a context switch.
    /// 
    /// Currently this only does two things:
    /// 1. Drops any data that the original previous task (before the context switch)
    ///    prepared for us to drop, as specified by `TaskInner::drop_after_task_switch`.
    /// 2. Obtains the preemption guard such that preemption can be re-enabled
    ///    when it is appropriate to do so.
    fn post_context_switch_action(&self) -> PreemptionGuard {
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

impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{{{}}}", self.name, self.id)
    }
}


/// Switches from the current task to the given `next` task.
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
    apic_id: u8,
    preemption_guard: PreemptionGuard,
) -> (bool, PreemptionGuard) {

    // We use the `with_current_task_and_value()` closure here in order to ensure that
    // the borrowed reference to the current task is guaranteed to be dropped
    // *before* the actual context switch operation occurs.
    let result = with_current_task_tls_slot_mut(
        |curr, p_guard| task_switch_inner(curr, next, apic_id, p_guard),
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
    #[cfg(not(simd_personality))]
    {
        call_context_switch!(context_switch::context_switch);
    }
    // If `simd_personality` is enabled, all `context_switch*` routines are available,
    // which allows us to choose one based on whether the prev/next Tasks are SIMD-enabled.
    #[cfg(simd_personality)]
    {
        let (curr_simd, next_simd) = (values_for_context_switch.2, values_for_context_switch.3);
        match (curr_simd, next_simd) {
            (SimdExt::None, SimdExt::None) => {
                // warn!("SWITCHING from REGULAR to REGULAR task");
                call_context_switch!(context_switch::context_switch_regular);
            }

            (SimdExt::None, SimdExt::SSE)  => {
                // warn!("SWITCHING from REGULAR to SSE task");
                call_context_switch!(context_switch::context_switch_regular_to_sse);
            }
            
            (SimdExt::None, SimdExt::AVX)  => {
                // warn!("SWITCHING from REGULAR to AVX task");
                call_context_switch!(context_switch::context_switch_regular_to_avx);
            }

            (SimdExt::SSE, SimdExt::None)  => {
                // warn!("SWITCHING from SSE to REGULAR task");
                call_context_switch!(context_switch::context_switch_sse_to_regular);
            }

            (SimdExt::SSE, SimdExt::SSE)   => {
                // warn!("SWITCHING from SSE to SSE task");
                call_context_switch!(context_switch::context_switch_sse);
            }

            (SimdExt::SSE, SimdExt::AVX) => {
                // warn!("SWITCHING from SSE to AVX task");
                call_context_switch!(context_switch::context_switch_sse_to_avx);
            }

            (SimdExt::AVX, SimdExt::None) => {
                // warn!("SWITCHING from AVX to REGULAR task");
                call_context_switch!(context_switch::context_switch_avx_to_regular);
            }

            (SimdExt::AVX, SimdExt::SSE) => {
                warn!("SWITCHING from AVX to SSE task");
                call_context_switch!(context_switch::context_switch_avx_to_sse);
            }

            (SimdExt::AVX, SimdExt::AVX) => {
                // warn!("SWITCHING from AVX to AVX task");
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

    let recovered_preemption_guard = with_current_task(|t|
        t.post_context_switch_action()
    ).expect("BUG: task_switch(): failed to get current task for post_context_switch_action");

    (true, recovered_preemption_guard)
}

#[cfg(not(simd_personality))]
type TaskSwitchInnerRet = (*mut usize, usize);
#[cfg(simd_personality)]
type TaskSwitchInnerRet = (*mut usize, usize, SimdExt, SimdExt);

/// The inner part of the task switching routine that modifies task states.
///
/// This accepts a mutably-borrowed reference to the current task's TLS variable
/// in order to potentially deinit that TLS variable (if the current task has exited).
/// Thus, it cannot perform the actual context switch operation,
/// because we cannot context switch until all `TaskRef`s on the current stack are dropped.
/// Hence, the th main [`task_switch()`] routine proceeds with the context switch
/// after we return to it from this function.
fn task_switch_inner(
    curr_task_tls_slot: &mut Option<TaskRef>,
    next: TaskRef,
    apic_id: u8,
    preemption_guard: PreemptionGuard,
) -> Result<TaskSwitchInnerRet, (bool, PreemptionGuard)> {
    let Some(ref curr) = curr_task_tls_slot else {
        error!("BUG: task_switch_inner(): couldn't get current task");
        return Err((false, preemption_guard));
    };

    // No need to task switch if the next task is the same as the current task.
    if curr.id == next.id {
        return Err((false, preemption_guard));
    }

    // trace!("task_switch [0]: (CPU {}) prev {:?}, next {:?}, interrupts?: {}", apic_id, curr, next, irq_safety::interrupts_enabled());

    // These conditions are checked elsewhere, but can be re-enabled if we want to be extra strict.
    // if !next.is_runnable() {
    //     error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was not Runnable! Current: {:?}, Next: {:?}", curr, next);
    //     return (false, preemption_guard);
    // }
    // if next.is_running() {
    //     error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was already running on AP {}!\nCurrent: {:?} Next: {:?}", apic_id, curr, next);
    //     return (false, preemption_guard);
    // }
    // if let Some(pc) = next.pinned_core() {
    //     if pc != apic_id {
    //         error!("BUG: Skipping task_switch due to scheduler bug: chosen 'next' Task was pinned to AP {:?} but scheduled on AP {}!\n\tCurrent: {:?}, Next: {:?}", pc, apic_id, curr, next);
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
        let mut inner = curr.inner.lock(); // ensure the lock is released
        (&mut inner.saved_sp) as *mut usize
    };
    let next_task_saved_sp: usize = {
        let inner = next.inner.lock(); // ensure the lock is released
        inner.saved_sp
    };

    // Mark the current task as no longer running
    curr.running_on_cpu.store(None.into());

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
    // We store the removed `TaskRef` in the next Task struct so that it remains accessible
    // until *after* the context switch.
    if curr_task_has_exited {
        // trace!("task_switch(): deiniting current task TLS for: {:?}, next: {}", curr_task_tls_slot.as_deref(), next.deref());
        let _prev_taskref = curr_task_tls_slot.take();
        next.inner.lock().drop_after_task_switch = _prev_taskref;
    }

    // Now, set the next task as the current task: the task running on this CPU.
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
        next.running_on_cpu.store(Some(apic_id).into());
        next.set_as_current_task();
        drop(_held_interrupts);
    }

    // Move the preemption guard into the next task such that we can use retrieve it
    // after the actual context switch operation has completed.
    //
    // TODO: this should be moved into per-CPU storage areas rather than the task struct.
    next.inner.lock().preemption_guard = Some(preemption_guard);

    #[cfg(not(simd_personality))]
    return Ok((prev_task_saved_sp, next_task_saved_sp));
    #[cfg(simd_personality)]
    return Ok((prev_task_saved_sp, next_task_saved_sp, curr_simd, next.simd));
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
/// [`join`]: JoinableTaskRef::join
//
// /// Note: this type is considered an internal implementation detail.
// /// Instead, use the `TaskJoiner` type from the `spawn` crate, 
// /// which is intended to be the public-facing interface for joining a task.
pub struct JoinableTaskRef {
    task: TaskRef,
}
assert_not_impl_any!(JoinableTaskRef: Clone);
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
    /// Busy-waits (spins in a loop) until this task has exited or has been killed.
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
        // First, wait for this Task to be marked as Exited (no longer runnable).
        while !self.has_exited() { }

        // Then, wait for it to actually stop running on any CPU core.
        while self.is_running() { }

        // This synchronizes with the release fence from when this task first ran
        // (in `spawn::task_wrapper()`).
        fence(Ordering::Acquire);

        self.reap_exit_value()
            .ok_or("BUG: `join()` could not retrieve `ExitValue` after task had exited.")
    }
}
impl Drop for JoinableTaskRef {
    /// Marks the inner [`Task`] as not joinable, meaning that it is an orphaned task
    /// that will be auto-reaped after exiting.
    fn drop(&mut self) {
        self.task.joinable.store(false, Ordering::Relaxed);
    }
}


/// A wrapper around `TaskRef` that allows this task to mark itself as exited.
///
/// This is only obtainable when a task is first switched to, specifically while
/// it is executing the `spawn::task_wrapper()` function
/// (before it proceeds to running its actual entry function).
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
    ///    prepared for us to drop, as specified by `TaskInner::drop_after_task_switch`.
    /// 2. Obtains the preemption guard such that preemption can be re-enabled
    ///    when it is appropriate to do so.
    ///
    /// Note: this publicly re-exports the private `TaskRef::post_context_switch_action()`
    ///       function for use in the early `spawn::task_wrapper` functions,
    ///       which is the only place where an `ExitableTaskRef` can be obtained. 
    pub fn post_context_switch_action(&self) -> PreemptionGuard {
        self.task.post_context_switch_action()
    }

    /// Allows the unwinder to obtain an `ExitableTaskRef` in order for it to
    /// be able to invoke this task's [`FailureCleanupFunction`].
    #[doc(hidden)]
    pub fn obtain_for_unwinder(current_task: TaskRef) -> Self {
        Self { task: current_task }
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
#[derive(Clone)]
pub struct TaskRef(Arc<TaskRefInner>);
impl fmt::Debug for TaskRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskRef")
            .field("task", &self.0.task)
            .finish_non_exhaustive()
    }
}
struct TaskRefInner {
    task: Task,
    exit_value_mailbox: Mutex<Option<ExitValue>>,
}

impl TaskRef {
    /// Creates a new `TaskRef`, a shareable wrapper around the given `Task`.
    /// 
    /// This does *not* add this task to the system-wide task list or any runqueues,
    /// nor does it schedule this task in.
    /// 
    /// ## Return
    /// Returns a [`JoinableTaskRef`], which derefs into the newly-created `TaskRef`
    /// and can be used to "join" this task (wait for it to exit) and obtain its exit value.
    pub fn create(task: Task) -> JoinableTaskRef {
        let exit_value_mailbox = Mutex::new(None);
        let taskref = TaskRef(Arc::new(TaskRefInner { task, exit_value_mailbox }));

        // Mark this task as joinable, now that it has been wrapped in the proper type.
        taskref.joinable.store(true, Ordering::Relaxed);
        JoinableTaskRef { task: taskref }
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
            self.runstate.store(RunState::Exited);

            // Corner case: if the task isn't currently running (as with killed tasks), 
            // we must clean it up now rather than in `task_switch()`, as it will never be scheduled in again.
            if !self.is_running() {
                todo!("Unhandled scenario: internal_exit(): task {:?} wasn't running \
                    but its current task TLS variable needs to be cleaned up!", &self.0.task);
                // Note: we cannot call `deinit_current_task()` here because if this task
                //       isn't running, then it's definitely not the current task.
                //
                // let _taskref_in_tls = deinit_current_task();
                // drop(_taskref_in_tls);
            }
            
            if let Some(waker) = self.inner.lock().waker.take() {
                waker.wake();
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
        if self.runstate.compare_exchange(RunState::Exited, RunState::Reaped).is_ok() {
            TASKLIST.lock().remove(&self.id);
            self.0.exit_value_mailbox.lock().take()
        } else {
            None
        }
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
        &self.0.task
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


/// Bootstrap a new task from the current thread of execution.
///
/// Returns a tuple of:
/// 1. a [`JoinableTaskRef`], which allows another task to join this bootstrapped task,
/// 2. an [`ExitableTaskRef`], which allows this bootstrapped task to mark itself
///    as exited once it has completed running.
///
/// ## Note
/// This function does not add the new task to any runqueue.
pub fn bootstrap_task(
    apic_id: u8, 
    stack: NoDrop<Stack>,
    kernel_mmi_ref: MmiRef,
) -> Result<(JoinableTaskRef, ExitableTaskRef), &'static str> {
    // Here, we cannot call `Task::new()` because tasking hasn't yet been set up for this core.
    // Instead, we generate all of the `Task` states manually, and create an initial task directly.
    let default_namespace = mod_mgmt::get_initial_kernel_namespace()
        .ok_or("The initial kernel CrateNamespace must be initialized before the tasking subsystem.")?
        .clone();
    let default_env = Arc::new(Mutex::new(Environment::default()));
    let mut bootstrap_task = Task::new_internal(
        stack.into_inner(),
        kernel_mmi_ref,
        default_namespace,
        default_env,
        None,
        bootstrap_task_cleanup_failure,
    );
    bootstrap_task.name = format!("bootstrap_task_core_{apic_id}");
    bootstrap_task.runstate.store(RunState::Runnable);
    bootstrap_task.running_on_cpu.store(Some(apic_id).into()); 
    bootstrap_task.inner.get_mut().pinned_core = Some(apic_id); // can only run on this CPU core
    let bootstrap_task_id = bootstrap_task.id;
    let joinable_taskref = TaskRef::create(bootstrap_task);

    // Set this task as this CPU's current task, as it's already running.
    joinable_taskref.set_as_current_task();
    let Ok(exitable_taskref) = init_current_task(
        bootstrap_task_id, 
        Some(joinable_taskref.clone()),
    ) else {
        error!("BUG: failed to set boostrapped task as current task on AP {}", apic_id);
        // Don't drop the bootstrap task upon error, because it contains the stack
        // used for the currently running code -- that would trigger an exception.
        let _task_ref = NoDrop::new(joinable_taskref);
        return Err("BUG: bootstrap_task(): failed to set bootstrapped task as current task");
    };

    // insert the new task into the task list
    let old_task = TASKLIST.lock().insert(bootstrap_task_id, joinable_taskref.clone());
    if let Some(ot) = old_task {
        error!("BUG: bootstrap_task(): TASKLIST already contained a task {:?} with the same id {} as bootstrap_task_core_{}!", 
            ot, bootstrap_task_id, apic_id
        );
        return Err("BUG: bootstrap_task(): TASKLIST already contained a task with the new bootstrap_task's ID");
    }
    
    Ok((joinable_taskref, exitable_taskref))
}


/// This is just like `spawn::task_cleanup_failure()`,
/// but for the initial tasks bootstrapped from each core's first execution context.
/// 
/// However, for a bootstrapped task, we don't know its function signature, argument type, or return value type
/// because it was invoked from assembly and may not even have one. 
/// 
/// Therefore there's not much we can actually do.
fn bootstrap_task_cleanup_failure(current_task: ExitableTaskRef, kill_reason: KillReason) -> ! {
    error!("BUG: bootstrap_task_cleanup_failure: {:?} died with {:?}\n. \
        There's nothing we can do here; looping indefinitely!",
        current_task,
        kill_reason,
    );
    loop { }
}


pub use tls_current_task::*;

/// A private module to ensure the below TLS variables aren't modified directly.
mod tls_current_task {
    use core::cell::{Cell, RefCell};
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
    /// Returns a `CurrentTaskNotFound`error if the current task cannot be obtained.
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
    ) -> Result<ExitableTaskRef, CurrentTaskAlreadyInited> {
        let taskref = if let Some(t) = current_task {
            if t.id != current_task_id {
                return Err(CurrentTaskAlreadyInited);
            }
            t
        } else {
            TASKLIST.lock()
                .get(&current_task_id)
                .cloned()
                .ok_or(CurrentTaskAlreadyInited)?
        };

        match CURRENT_TASK.try_borrow_mut() {
            Ok(mut t_opt) if t_opt.is_none() => {
                *t_opt = Some(taskref.clone());
                CURRENT_TASK_ID.set(current_task_id);
                Ok(ExitableTaskRef { task: taskref })
            }
            _ => Err(CurrentTaskAlreadyInited),
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
        F: FnOnce(&mut Option<TaskRef>, T) -> R
    {
        if let Ok(tls_slot) = CURRENT_TASK.try_borrow_mut().as_deref_mut() {
            Ok(function(tls_slot, value))
        } else {
            Err(value)
        }
    }

    /// An error type indicating that the current task was already initialized.
    #[derive(Debug)]
    pub struct CurrentTaskAlreadyInited;
    /// An error type indicating that the current task has not yet been initialized.
    #[derive(Debug)]
    pub struct CurrentTaskNotFound;
}
