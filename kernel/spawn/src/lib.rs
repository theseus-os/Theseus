//! This crate offers routines for spawning new tasks
//! and convenient builder patterns for customizing new tasks. 
//! 
//! The two functions of interest to create a `TaskBuilder` are:
//! * [`new_task_builder()`][tb]:  creates a new task for a known, existing function.
//! * [`new_application_task_builder()`][atb]: loads a new application crate and creates a new task
//!    for that crate's entry point (main) function.
//! 
//! [tb]:  fn.new_task_builder.html
//! [atb]: fn.new_application_task_builder.html

#![no_std]
#![feature(stmt_expr_attributes)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate debugit;
extern crate irq_safety;
extern crate memory;
extern crate stack;
extern crate task;
extern crate runqueue;
extern crate scheduler;
extern crate mod_mgmt;
extern crate apic;
extern crate context_switch;
extern crate path;
extern crate fs_node;
extern crate catch_unwind;
extern crate fault_crate_swap;
extern crate pause;
extern crate spin;
extern crate thread_local_macro;


use core::{marker::PhantomData, mem, ops::Deref};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;
use irq_safety::{MutexIrqSafe, hold_interrupts, enable_interrupts};
use memory::{get_kernel_mmi_ref, MemoryManagementInfo};
use stack::Stack;
use task::{Task, TaskRef, get_my_current_task, RestartInfo, TASKLIST, JoinableTaskRef};
use mod_mgmt::{CrateNamespace, SectionType, SECTION_HASH_DELIMITER};
use path::Path;
use apic::get_my_apic_id;
use fs_node::FileOrDir;

#[cfg(simd_personality)]
use task::SimdExt;


/// Initializes tasking for the given AP core, including creating a runqueue for it
/// and creating its initial task bootstrapped from the current execution context for that core. 
pub fn init(
    kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>,
    apic_id: u8,
    stack: Stack,
) -> Result<BootstrapTaskRef, &'static str> {
    runqueue::init(apic_id)?;
    
    let task_ref = task::bootstrap_task(apic_id, stack, kernel_mmi_ref)?;
    BOOTSTRAP_TASKS.lock().push(task_ref.clone());
    runqueue::add_task_to_specific_runqueue(apic_id, task_ref.clone())?;
    Ok(BootstrapTaskRef {
        apic_id, 
        task_ref,
    })
}

/// The set of bootstrap tasks that are created using `task::bootstrap_task()`.
/// These require special cleanup; see [`cleanup_bootstrap_tasks()`].
static BOOTSTRAP_TASKS: Mutex<Vec<TaskRef>> = Mutex::new(Vec::new());

/// Spawns a dedicated task to cleanup all bootstrap tasks
/// by reaping them, i.e., taking their exit value.
/// 
/// This allows them to be fully dropped and cleaned up safely,
/// as it would be invalid to reap and cleanup bootstrap tasks
/// while the actual bootstrapped task was still running.
/// 
/// ## Arguments
/// * `num_tasks`: the number of bootstrap tasks that must be cleaned up.
pub fn cleanup_bootstrap_tasks(num_tasks: usize) -> Result<(), &'static str> {
    new_task_builder(
        |total_tasks: usize| {
            let mut num_tasks_cleaned = 0;
            while num_tasks_cleaned < total_tasks {
                if let Some(task) = BOOTSTRAP_TASKS.lock().pop() {
                    task.join().unwrap();
                    if let Some(_) = task.take_exit_value() {
                        // trace!("Cleaned up bootstrap task {:?}", task);
                        num_tasks_cleaned += 1;
                    } else {
                        panic!("BUG: bootstrap task didn't exit before cleanup: {:?}", task);
                    }
                }
            }
            info!("Cleaned up all {} bootstrap tasks.", total_tasks);
            *BOOTSTRAP_TASKS.lock() = Vec::new(); // replace the Vec to drop it
        },
        num_tasks,
    )
    .name(String::from("bootstrap_task_cleanup"))
    .spawn()?;

    Ok(())
}

/// A wrapper around a `TaskRef` for bootstrapped tasks, which are the tasks
/// that represent the first thread of execution on each CPU when it first boots.
/// 
/// When a bootstrap task has done everything it needs to do, 
/// it should invoke [`BootstrapTaskRef::finish()`] to indicate that it's finished,
/// which will then mark itself as exited and remove itself from runqueues.
/// 
/// See [`init()`] and [`task::bootstrap_task()`].
#[derive(Debug)]
pub struct BootstrapTaskRef {
    #[allow(dead_code)]
    apic_id: u8,
    task_ref: JoinableTaskRef,
}
impl Deref for BootstrapTaskRef {
    type Target = TaskRef;
    fn deref(&self) -> &TaskRef {
        &self.task_ref
    }
}
impl BootstrapTaskRef {
    /// This function represents the final step of each CPU's initialization procedure.
    /// 
    /// This function does the following:
    /// 1. Consumes this bootstrap task such that it can no longer be accessed.
    /// 2. Marks this bootstrap task as exited.
    /// 3. Removes this bootstrap task from all this CPU's runqueue.
    /// 
    /// This function consumes this bootstrap task, marks it as exited
    pub fn finish(self) {
        drop(self);
    }
}
impl Drop for BootstrapTaskRef {
    // See the documentation for `BootstrapTaskRef::finish()` for more details.
    fn drop(&mut self) {
        // trace!("Finishing Bootstrap Task on core {}: {:?}", self.apic_id, self.task_ref);
        remove_current_task_from_runqueue(&self.task_ref);
        self.mark_as_exited(Box::new(()))
            .expect("BUG: bootstrap task was unable to mark itself as exited");

        // Note: we can mark this bootstrap task as exited here, but we cannot 
        // reap it (take its exit value) safely because it might be currently running.
        // Doing so would cause its stack to be deallocated and the current execution to fail.
        // Instead, that is done in `cleanup_bootstrap_tasks()`.
    }
}


/// Creates a builder for a new `Task` that starts at the given entry point function `func`
/// and will be passed the given `argument`.
/// 
/// # Note 
/// The new task will not be spawned until [`TaskBuilder::spawn()`](struct.TaskBuilder.html#method.spawn) is invoked. 
/// See the `TaskBuilder` documentation for more details. 
/// 
pub fn new_task_builder<F, A, R>(
    func: F,
    argument: A
) -> TaskBuilder<F, A, R>
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R,
{
    TaskBuilder::new(func, argument)
}


/// Every executable application must have an entry function named "main".
const ENTRY_POINT_SECTION_NAME: &'static str = "main";

/// The argument type accepted by the `main` function entry point into each application.
type MainFuncArg = Vec<String>;

/// The type returned by the `main` function entry point of each application.
type MainFuncRet = isize;

/// The function signature of the `main` function that every application must have,
/// as it is the entry point into each application `Task`.
type MainFunc = fn(MainFuncArg) -> MainFuncRet;

/// Creates a builder for a new application `Task`. 
/// 
/// The new task will start at the application crate's entry point `main` function.
/// 
/// Note that the application crate will be loaded and linked during this function,
/// but the actual new application task will not be spawned until [`TaskBuilder::spawn()`](struct.TaskBuilder.html#method.spawn) is invoked.
/// 
/// # Arguments
/// * `crate_object_file`: the object file that the application crate will be loaded from.
/// * `new_namespace`: if provided, the new application task will be spawned within the new `CrateNamespace`,
///    meaning that the new application crate will be linked against the crates within that new namespace. 
///    If not provided, the new Task will be spawned within the same namespace as the current task.
/// 
pub fn new_application_task_builder(
    crate_object_file: Path, // TODO FIXME: use `mod_mgmt::IntoCrateObjectFile`,
    new_namespace: Option<Arc<CrateNamespace>>,
) -> Result<TaskBuilder<MainFunc, MainFuncArg, MainFuncRet>, &'static str> {
    
    let namespace = new_namespace.or_else(|| task::get_my_current_task().map(|t| t.get_namespace().clone()))
        .ok_or("spawn::new_application_task_builder(): couldn't get current task to use its CrateNamespace")?;
    
    let crate_object_file = match crate_object_file.get(namespace.dir())
        .or_else(|| Path::new(format!("{}.o", &crate_object_file)).get(namespace.dir())) // retry with ".o" extension
    {
        Some(FileOrDir::File(f)) => f,
        _ => return Err("Couldn't find specified file path for new application crate"),
    };
    
    // Load the new application crate
    let app_crate_ref = {
        let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("couldn't get_kernel_mmi_ref")?;
        CrateNamespace::load_crate_as_application(&namespace, &crate_object_file, &kernel_mmi_ref, false)?
    };

    // Find the "main" entry point function in the new app crate
    let main_func_sec_opt = { 
        let app_crate = app_crate_ref.lock_as_ref();
        let expected_main_section_name = format!("{}{}{}", app_crate.crate_name_as_prefix(), ENTRY_POINT_SECTION_NAME, SECTION_HASH_DELIMITER);
        app_crate.find_section(|sec| 
            sec.get_type() == SectionType::Text && sec.name_without_hash() == &expected_main_section_name
        ).cloned()
    };
    let main_func_sec = main_func_sec_opt.ok_or("spawn::new_application_task_builder(): couldn't find \"main\" function, expected function name like \"<crate_name>::main::<hash>\"\
        --> Is this an app-level library or kernel crate? (Note: you cannot spawn a library crate with no main function)")?;
    // SAFETY: None. There is a lint in compiler_plugins/application_main_fn.rs, but it's currently disabled.
    let main_func = unsafe { main_func_sec.as_func::<MainFunc>() }?;

    // Create the underlying task builder. 
    // Give it a default name based on the app crate's name, but that can be changed later. 
    let mut tb = TaskBuilder::new(*main_func, MainFuncArg::default())
        .name(app_crate_ref.lock_as_ref().crate_name.to_string()); 

    // Once the new application task is created (but before its scheduled in),
    // ensure it has the relevant app-specific fields set properly.
    tb.post_build_function = Some(Box::new(
        move |new_task| {
            new_task.app_crate = Some(Arc::new(app_crate_ref));
            new_task.namespace = namespace;
            Ok(())
        }
    ));
    
    Ok(tb)
}

/// A struct that offers a builder pattern to create and customize new `Task`s.
/// 
/// Note that the new `Task` will not actually be created until [`spawn()`](struct.TaskBuilder.html#method.spawn) is invoked.
/// 
/// To create a `TaskBuilder`, use these functions:
/// * [`new_task_builder()`][tb]:  creates a new task for a known, existing function.
/// * [`new_application_task_builder()`][atb]: loads a new application crate and creates a new task
///    for that crate's entry point (main) function.
/// 
/// [tb]:  fn.new_task_builder.html
/// [atb]: fn.new_application_task_builder.html
pub struct TaskBuilder<F, A, R> {
    func: F,
    argument: A,
    _return_type: PhantomData<R>,
    name: Option<String>,
    pin_on_core: Option<u8>,
    blocked: bool,
    idle: bool,
    post_build_function: Option<Box< dyn FnOnce(&mut Task) -> Result<(), &'static str> >>,

    #[cfg(simd_personality)]
    simd: SimdExt,
}

impl<F, A, R> TaskBuilder<F, A, R> 
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R,
{
    /// Creates a new `Task` from the given function `func`
    /// that will be passed the argument `arg` when spawned. 
    fn new(func: F, argument: A) -> TaskBuilder<F, A, R> {
        TaskBuilder {
            argument: argument,
            func: func,
            _return_type: PhantomData,
            name: None,
            pin_on_core: None,
            blocked: false,
            idle: false,
            post_build_function: None,

            #[cfg(simd_personality)]
            simd: SimdExt::None,
        }
    }

    /// Set the String name for the new Task.
    pub fn name(mut self, name: String) -> TaskBuilder<F, A, R> {
        self.name = Some(name);
        self
    }

    /// Set the argument that will be passed to the new Task's entry function.
    pub fn argument(mut self, argument: A) -> TaskBuilder<F, A, R> {
        self.argument = argument;
        self
    }

    /// Pin the new Task to a specific core.
    pub fn pin_on_core(mut self, core_apic_id: u8) -> TaskBuilder<F, A, R> {
        self.pin_on_core = Some(core_apic_id);
        self
    }

    /// Mark this new Task as a SIMD-enabled Task 
    /// that can run SIMD instructions and use SIMD registers.
    #[cfg(simd_personality)]
    pub fn simd(mut self, extension: SimdExt) -> TaskBuilder<F, A, R> {
        self.simd = extension;
        self
    }

    /// Set the new Task's `RunState` to be `Blocked` instead of `Runnable` when it is first spawned.
    /// This allows another task to delay the new task's execution arbitrarily, 
    /// e.g., to set up other things for the newly-spawned (but not yet running) task. 
    /// 
    /// Note that the new Task will not be `Runnable` until it is explicitly set as such.
    pub fn block(mut self) -> TaskBuilder<F, A, R> {
        self.blocked = true;
        self
    }

    /// Finishes this `TaskBuilder` and spawns the new task as described by its builder functions.
    /// 
    /// This merely makes the new task Runnable, it does not switch to it immediately; that will happen on the next scheduler invocation.
    #[inline(never)]
    pub fn spawn(self) -> Result<JoinableTaskRef, &'static str> {
        let mut new_task = Task::new(
            None,
            task_cleanup_failure::<F, A, R>,
        )?;
        // If a Task name wasn't provided, then just use the function's name.
        new_task.name = self.name.unwrap_or_else(|| String::from(core::any::type_name::<F>()));
    
        #[cfg(simd_personality)] {  
            new_task.simd = self.simd;
        }

        setup_context_trampoline(&mut new_task, task_wrapper::<F, A, R>)?;

        // Currently we're using the very bottom of the kstack for kthread arguments. 
        // This is probably stupid (it'd be best to put them directly where they need to go towards the top of the stack),
        // but it simplifies type safety in the `task_wrapper` entry point and removes uncertainty from assumed calling conventions.
        let bottom_of_stack: &mut usize = new_task.inner_mut().kstack.as_type_mut(0)?;
        let box_ptr = Box::into_raw(Box::new(TaskFuncArg::<F, A, R> {
            arg:  self.argument,
            func: self.func,
            _rettype: PhantomData,
        }));
        *bottom_of_stack = box_ptr as usize;

        // The new task is ready to be scheduled in, now that its stack trampoline has been set up.
        if self.blocked {
            new_task.block();
        } else {
            new_task.unblock();
        }

        // The new task is marked as idle
        if self.idle {
            new_task.is_an_idle_task = true;
        }

        // If there is a post-build function, invoke it now before finalizing the task and adding it to runqueues.
        if let Some(pb_func) = self.post_build_function {
            pb_func(&mut new_task)?;
        }

        let new_task_id = new_task.id;
        let task_ref = TaskRef::new(new_task);
        let _existing_task = TASKLIST.lock().insert(new_task_id, task_ref.clone());
        // insert should return None, because that means there was no existing task with the same ID 
        if let Some(_existing_task) = _existing_task {
            error!("BUG: TaskBuilder::spawn(): Fatal Error: TASKLIST already contained a task with the new task's ID! {:?}", _existing_task);
            return Err("BUG: TASKLIST a contained a task with the new task's ID");
        }
        
        if let Some(core) = self.pin_on_core {
            runqueue::add_task_to_specific_runqueue(core, task_ref.clone())?;
        } else {
            runqueue::add_task_to_any_runqueue(task_ref.clone())?;
        }

        Ok(task_ref)

        // Ok(TaskJoiner::<R> {
        //     task: task_ref,
        //     _phantom: PhantomData,
        // })
    }

}

/// Additional implementation of `TaskBuilder` to be used for 
/// restartable functions. Further restricts the function (F) 
/// and argument (A) to implement `Clone` trait.
impl<F, A, R> TaskBuilder<F, A, R> 
    where A: Send + Clone + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R + Send + Clone +'static,
{
    /// Sets this new Task to be the idle task for the given core. 
    /// 
    /// Idle tasks will not be scheduled unless there are no other tasks for the scheduler to choose. 
    /// 
    /// Idle tasks must be restartable, so it is only a possible option when spawning a restartable task.
    /// Marking a task as idle is only needed to set up one for each core when that core is initialized,
    /// but or to restart an idle task that has exited or failed.
    /// 
    /// There is no harm spawning multiple idle tasks on each core, but it's a waste of space. 
    pub fn idle(mut self, core_id: u8) -> TaskBuilder<F, A, R> {
        self.idle = true;
        self.pin_on_core(core_id)
    }

    /// Like [`TaskBuilder::spawn()`], this finishes this `TaskBuilder` and spawns the new task.
    /// It also stores the new Task's function and argument within the Task,
    /// enabling it to be restarted upon exit.
    /// 
    /// ## Arguments
    /// * `restart_with_arg`: if `Some`, this argument will be passed into the restarted task
    ///    instead of the argument initially provided to [`new_task_builder()`].
    /// 
    /// Note that the argument initially provided to `new_task_builder()` will *always*
    /// be passed into the initially-spawned instance of this task.
    /// The `restart_with_arg` value is only used as an argument for *future* instances
    /// of this task that are re-spawned (restarted) if the initial task exits.
    /// 
    /// This allows one to spawn a task that is restartable but performs a given action
    /// with its initial argument only once.
    /// This is typically achieved by using an `Option<T>` for the argument type `A`:
    /// * The argument `Some(T)` is passed into `new_task_builder()`,
    ///   such that it is used for and passed to the first spawned instance of this task.
    /// * The argument `None` is used for `restart_with_arg`,
    ///   such that it is used for and passed to the subsequent restarted instances of this task.
    /// 
    /// This function merely makes the new task Runnable, it does not switch to it immediately;
    /// that will happen on the next scheduler invocation.
    #[inline(never)]
    pub fn spawn_restartable(
        mut self,
        restart_with_arg: Option<A>
    ) -> Result<JoinableTaskRef, &'static str> {
        let restart_info = RestartInfo {
            argument: Box::new(restart_with_arg.unwrap_or_else(|| self.argument.clone())),
            func: Box::new(self.func.clone()),
        };

        // Once the new task is created, we set its restart info (func and arg),
        // and tell it to use the restartable version of the final task cleanup function.
        self.post_build_function = Some(Box::new(
            move |new_task| {
                new_task.inner_mut().restart_info = Some(restart_info);
                new_task.failure_cleanup_function = task_restartable_cleanup_failure::<F, A, R>;
                setup_context_trampoline(new_task, task_wrapper_restartable::<F, A, R>)?;
                Ok(())
            }
        ));

        // Code path is shared between `spawn` and `spawn_restartable` from this point
        self.spawn()
    }
}


// Note: this is currently not used because it requires many sweeping changes
//       everywhere that `spawn()` is called to pass on the generic type parameter `R`.
//
// /// The object is returned when a new [`Task`] is [`spawn`]ed.
// /// 
// /// This allows the "parent" task (the one that spawned this task) to:
// /// * [`join`] this task, i.e., wait for this task to finish executing,
// /// * to obtain its [exit value] after it has completed.
// /// 
// /// The type parameter `R` is the type that this task will return upon successful completion.
// /// As such, it is derived from the return type of the entry function `func`
// /// that was passed into [`new_task_builder()`]
// /// If dropped, this task will be *detached* and treated as an "orphan" task.
// /// This means that there is no way for another task to wait for it to complete
// /// or obtain its exit value.
// /// As such, this task will be auto-reaped after it exits (in order to avoid zombie tasks).
// /// 
// /// Implementation-wise, this is a wrapper around [`JoinableTaskRef`], which marks a task
// /// as non-joinable when it is dropped.
// /// This type adds the ability to obtain its exit value as a typed object, 
// /// because only the [`spawn`] function knows that type `R`, whereas the task itself does not.
// /// 
// /// [`spawn`]: TaskBuilder::spawn
// /// [`join`]: TaskRef::join
// /// [exit value]: task::ExitValue
// pub struct TaskJoiner<R: Send + 'static> {
//     task: JoinableTaskRef,
//     _phantom: PhantomData<R>,
// }
// impl<R: Send + 'static> Deref for TaskJoiner<R> {
//     type Target = JoinableTaskRef;
//     fn deref(&self) -> &Self::Target {
//         &self.task
//     }
// }


/// A wrapper around a task's function and argument.
#[derive(Debug)]
struct TaskFuncArg<F, A, R> {
    func: F,
    arg:  A,
    // not necessary, just for consistency in "<F, A, R>" signatures.
    _rettype: PhantomData<*const R>,
}


/// This function sets up the given new `Task`'s kernel stack pointer to properly jump
/// to the given entry point function when the new `Task` is first scheduled in. 
/// 
/// When a new task is first scheduled in, a `Context` struct will be popped off the stack,
/// and at the end of that struct is the address of the next instruction that will be popped off as part of the "ret" instruction, 
/// i.e., the entry point into the new task. 
/// 
/// So, this function allocates space for the saved context registers to be popped off when this task is first switched to.
/// It also sets the given `new_task`'s `saved_sp` (its saved stack pointer, which holds the Context for task switching).
/// 
fn setup_context_trampoline(new_task: &mut Task, entry_point_function: fn() -> !) -> Result<(), &'static str> {
    
    /// A private macro that actually creates the Context and sets it up in the `new_task`.
    /// We use a macro here so we can pass in the proper `ContextType` at runtime, 
    /// which is useful for both the simd_personality config and regular/SSE configs.
    macro_rules! set_context {
        ($ContextType:ty) => (
            // We write the new Context struct at the top of the stack, which is at the end of the stack's MappedPages. 
            // We subtract "size of usize" (8) bytes to ensure the new Context struct doesn't spill over past the top of the stack.
            let mp_offset = new_task.inner_mut().kstack.size_in_bytes() - mem::size_of::<usize>() - mem::size_of::<$ContextType>();
            let new_context_destination: &mut $ContextType = new_task.inner_mut().kstack.as_type_mut(mp_offset)?;
            *new_context_destination = <$ContextType>::new(entry_point_function as usize);
            new_task.inner_mut().saved_sp = new_context_destination as *const _ as usize;
        );
    }

    // If `simd_personality` is enabled, all of the `context_switch*` implementation crates are simultaneously enabled,
    // in order to allow choosing one of them based on the configuration options of each Task (SIMD, regular, etc).
    #[cfg(simd_personality)] {
        match new_task.simd {
            SimdExt::AVX => {
                // warn!("USING AVX CONTEXT for Task {:?}", new_task);
                set_context!(context_switch::ContextAVX);
            }
            SimdExt::SSE => {
                // warn!("USING SSE CONTEXT for Task {:?}", new_task);
                set_context!(context_switch::ContextSSE);
            }
            SimdExt::None => {
                // warn!("USING REGULAR CONTEXT for Task {:?}", new_task);
                set_context!(context_switch::ContextRegular);
            }
        }
    }

    // If `simd_personality` is NOT enabled, then we use the context_switch routine that matches the actual build target. 
    #[cfg(not(simd_personality))] {
        // The context_switch crate exposes the proper TARGET-specific `Context` type here.
        set_context!(context_switch::Context);
    }

    Ok(())
}

/// Internal code of `task_wrapper` shared by `task_wrapper` and 
/// `task_wrapper_restartable`. 
fn task_wrapper_internal<F, A, R>() -> Result<R, task::KillReason>
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    let task_entry_func;
    let task_arg;

    // This is scoped to ensure that absolutely no resources that require dropping are held
    // when invoking the task's entry function, in order to simplify cleanup when unwinding.
    // *No* local variables should exist on the stack at the end of this function,
    // except for the task's `func` and `arg`, which are obviously required.
    {
        let curr_task = get_my_current_task().expect("BUG: task_wrapper: couldn't get current task (before task func).");

        // The first time that a task runs, its entry function `task_wrapper()` is jumped to
        // from the `task_switch()` function, right after the end of `context_switch`(),
        // Thus, the first thing we must do here is to perform post-context switch actions,
        // because this is the first code to run immediately after a context switch
        // switches to this task for the first time.
        // For more details, see the comments at the end of `Task::task_switch()`.
        curr_task.post_context_switch_action();

        // This task's function and argument were placed at the bottom of the stack when this task was spawned.
        let task_func_arg = curr_task.with_kstack(|kstack| {
            kstack.as_type(0).map(|tfa_box_raw_ptr: &usize| {
                // SAFE: we placed this Box in this task's stack in the `spawn()` function when creating the TaskFuncArg struct.
                let tfa_boxed = unsafe { Box::from_raw((*tfa_box_raw_ptr) as *mut TaskFuncArg<F, A, R>) };
                *tfa_boxed // un-box it
            })
        }).expect("BUG: task_wrapper: couldn't access task's function/argument at bottom of stack");
        task_entry_func = task_func_arg.func;
        task_arg  = task_func_arg.arg;

        #[cfg(not(any(rq_eval, downtime_eval)))]
        debug!("task_wrapper [1]: \"{}\" about to call task entry func {:?} {{{}}} with arg {:?}",
            curr_task.name.clone(), debugit!(task_entry_func), core::any::type_name::<F>(), debugit!(task_arg)
        );
    };

    // The first time that a task runs, its entry function `task_wrapper()` is jumped to
    // from the `task_switch()` function, right after the context switch occurred.
    // Since the `task_switch()` function was originally invoked from an interrupt handler,
    // interrupts were disabled but never had the chance to be re-enabled
    // because we did not return from the interrupt handler as usual.
    // Therefore, we need to re-enabled interrupts here to ensure that things continue
    // to run as normal, with interrupts enabled so we can properly preempt this task.
    enable_interrupts();

    // Now we actually invoke the entry point function that this Task was spawned for, catching a panic if one occurs.
    catch_unwind::catch_unwind_with_arg(task_entry_func, task_arg)
}

/// The entry point for all new `Task`s except restartable tasks. 
/// This does not return, because it doesn't really have anywhere to return.
fn task_wrapper<F, A, R>() -> !
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    let result = task_wrapper_internal::<F, A, R>();

    // Here: now that the task is finished running, we must clean in up by doing three things:
    // 1. Put the task into a non-runnable mode (exited or killed) and set its exit value or killed reason
    // 2. Remove it from its runqueue
    // 3. Yield the CPU
    //
    // The first two need to be done "atomically" (without interruption), so we must disable preemption before step 1.
    // Otherwise, this task could be marked as `Exited`, and then a context switch could occur to another task,
    // which would prevent this task from ever running again, so it would never get to remove itself from the runqueue.
    //
    // Operations 1 happen in `task_cleanup_success` or `task_cleanup_failure`, 
    // while operations 2 and 3 then happen in `task_cleanup_final`.
    let curr_task = get_my_current_task().expect("BUG: task_wrapper: couldn't get current task (after task func).").clone();
    match result {
        Ok(exit_value)   => task_cleanup_success::<F, A, R>(curr_task, exit_value),
        Err(kill_reason) => task_cleanup_failure::<F, A, R>(curr_task, kill_reason),
    }
}

/// Similar to `task_wrapper` in functionality but used as entry point only for 
/// restartable tasks. Further restricts `argument` to implement `Clone` trait. 
/// // We cannot use `task_wrapper` as it is not bounded by `Clone` trait.
fn task_wrapper_restartable<F, A, R>() -> !
    where A: Send + Clone + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R + Send + Clone + 'static,
{
    let result = task_wrapper_internal::<F, A, R>();

    // See `task_wrapper` for an explanation of how the below functions work.
    let curr_task = get_my_current_task().expect("BUG: task_wrapper: couldn't get current task (after task func).").clone();
    match result {
        Ok(exit_value)   => task_restartable_cleanup_success::<F, A, R>(curr_task, exit_value),
        Err(kill_reason) => task_restartable_cleanup_failure::<F, A, R>(curr_task, kill_reason),
    }
}



/// Internal function cleans up a task that exited properly. 
/// Contains the shared code between `task_cleanup_success` and `task_cleanup_success_restartable`
#[inline(always)]
fn task_cleanup_success_internal<R>(current_task: TaskRef, exit_value: R) -> (irq_safety::HeldInterrupts, TaskRef)
    where R: Send + 'static,
{ 
    // Disable preemption (currently just disabling interrupts altogether)
    let held_interrupts = hold_interrupts();

    #[cfg(not(rq_eval))]
    debug!("task_cleanup_success: {:?} successfully exited with return value {:?}", current_task.name, debugit!(exit_value));
    if current_task.mark_as_exited(Box::new(exit_value)).is_err() {
        error!("task_cleanup_success: {:?} task could not set exit value, because task had already exited. Is this correct?", current_task.name);
    }

    (held_interrupts, current_task)
}

/// This function cleans up a task that exited properly.
fn task_cleanup_success<F, A, R>(current_task: TaskRef, exit_value: R) -> !
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{   
    let (held_interrupts, current_task) = task_cleanup_success_internal(current_task, exit_value);
    task_cleanup_final::<F, A, R>(held_interrupts, current_task)
}

/// Similar to `task_cleanup_success` but used on restartable_tasks
fn task_restartable_cleanup_success<F, A, R>(current_task: TaskRef, exit_value: R) -> !
    where A: Send + Clone + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R + Send + Clone +'static,
{
    let (held_interrupts, current_task) = task_cleanup_success_internal(current_task, exit_value);
    task_restartable_cleanup_final::<F, A, R>(held_interrupts, current_task)
}



/// Internal function that cleans up a task that did not exit properly.
#[inline(always)]
fn task_cleanup_failure_internal(current_task: TaskRef, kill_reason: task::KillReason) -> (irq_safety::HeldInterrupts, TaskRef) {
    // Disable preemption (currently just disabling interrupts altogether)
    let held_interrupts = hold_interrupts();

    #[cfg(not(downtime_eval))]
    debug!("task_cleanup_failure: {:?} panicked with {:?}", current_task.name, kill_reason);

    if current_task.mark_as_killed(kill_reason).is_err() {
        error!("task_cleanup_failure: {:?} task could not set kill reason, because task had already exited. Is this correct?", current_task.name);
    }

    (held_interrupts, current_task)
}     

/// This function cleans up a task that did not exit properly,
/// e.g., it panicked, hit an exception, etc. 
/// 
/// A failure that occurs while unwinding a task will also jump here.
/// 
/// The generic type parameters are derived from the original `task_wrapper` invocation,
/// and are here to provide type information needed when cleaning up a failed task.
fn task_cleanup_failure<F, A, R>(current_task: TaskRef, kill_reason: task::KillReason) -> !
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    let (held_interrupts, current_task) = task_cleanup_failure_internal(current_task, kill_reason);
    task_cleanup_final::<F, A, R>(held_interrupts, current_task)
}

/// Similar to `task_cleanup_failure` but used on restartable_tasks
fn task_restartable_cleanup_failure<F, A, R>(current_task: TaskRef, kill_reason: task::KillReason) -> !
    where A: Send + Clone + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R + Send + Clone + 'static, 
{
    let (held_interrupts, current_task) = task_cleanup_failure_internal(current_task, kill_reason);
    task_restartable_cleanup_final::<F, A, R>(held_interrupts, current_task)
}


/// Internal function that performs final cleanup actions for an exited task.
#[inline(always)]
fn task_cleanup_final_internal(current_task: &TaskRef) {
    // First, remove the task from its runqueue(s).
    remove_current_task_from_runqueue(current_task);

    // Second, run TLS object destructors, which will drop any TLS objects
    // that were lazily initialized during this execution of this task.
    for tls_dtor in thread_local_macro::take_current_tls_destructors().into_iter() {
        unsafe {
            (tls_dtor.dtor)(tls_dtor.object_ptr as *mut u8);
        }
    }

    // Third, reap the task if it has been orphaned (if it's non-joinable).
    if !current_task.is_joinable() {
        // trace!("Reaping orphaned task... {:?}", current_task);
        let _exit_value = current_task.take_exit_value();
        // trace!("Reaped orphaned task {:?}, {:?}", current_task, _exit_value);
    }
}


/// The final piece of the task cleanup logic,
/// which removes the task from its runqueue and permanently deschedules it. 
fn task_cleanup_final<F, A, R>(held_interrupts: irq_safety::HeldInterrupts, current_task: TaskRef) -> ! 
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    task_cleanup_final_internal(&current_task);
    drop(current_task);
    drop(held_interrupts);
    // ****************************************************
    // NOTE: nothing below here is guaranteed to run again!
    // ****************************************************

    scheduler::schedule();
    error!("BUG: task_cleanup_final(): task was rescheduled after being dead!");
    loop { }
}

/// The final piece of the task cleanup logic for restartable tasks.
/// which removes the task from its runqueue and spawns it again with 
/// same entry function (F) and argument (A). 
fn task_restartable_cleanup_final<F, A, R>(held_interrupts: irq_safety::HeldInterrupts, current_task: TaskRef) -> ! 
   where A: Send + Clone + 'static, 
         R: Send + 'static,
         F: FnOnce(A) -> R + Send + Clone + 'static, 
{
    task_cleanup_final_internal(&current_task);

    {
        #[cfg(use_crate_replacement)]
        let mut se = fault_crate_swap::SwapRanges::default();

        // Get the crate we should swap. Will be None if nothing is picked
        #[cfg(use_crate_replacement)] {
            if let Some(crate_to_swap) = fault_crate_swap::get_crate_to_swap() {
                // Call the handler to swap the crates
                let version = fault_crate_swap::self_swap_handler(&crate_to_swap);
                match version {
                    Ok(v) => {
                        se = v
                    }
                    Err(err) => {
                        debug!(" Crate swapping failed {:?}", err)
                    }
                }
            }
        }

        // Re-spawn a new instance of the task if it was spawned as a restartable task. 
        // We must not hold the current task's lock when calling spawn().
        let restartable_info = current_task.with_restart_info(|restart_info_opt| {
            restart_info_opt.map(|restart_info| {
                #[cfg(use_crate_replacement)] {
                    let func_ptr = &(restart_info.func) as *const _ as usize;
                    let arg_ptr = &(restart_info.argument) as *const _ as usize;

                    #[cfg(not(downtime_eval))] {
                        debug!("func_ptr {:#X}", func_ptr);
                        debug!("arg_ptr {:#X} , {}", arg_ptr, mem::size_of::<A>());
                    }

                    // func_ptr is of size 16. Argument is of the argument_size + 8.
                    // This extra size comes due to argument and function both stored in +8 location pointed by the pointer. 
                    // The exact location pointed by the pointer has value 0x1. (Indicates Some for option ?). 
                    if fault_crate_swap::constant_offset_fix(&se, func_ptr, func_ptr + 16).is_ok() &&  fault_crate_swap::constant_offset_fix(&se, arg_ptr, arg_ptr + 8).is_ok() {
                        #[cfg(not(downtime_eval))]
                        debug!("Function and argument addresses corrected");
                    }
                }

                let func: &F = restart_info.func.downcast_ref().expect("BUG: failed to downcast restartable task's function");
                let arg : &A = restart_info.argument.downcast_ref().expect("BUG: failed to downcast restartable task's argument");
                (func.clone(), arg.clone())
            })
        });

        if let Some((func, arg)) = restartable_info {
            let mut new_task = new_task_builder(func, arg)
                .name(current_task.name.clone());
            if let Some(core) = current_task.pinned_core() {
                new_task = new_task.pin_on_core(core);
            }
            new_task.spawn_restartable(None)
                .expect("Failed to respawn the restartable task");
        } else {
            error!("BUG: Restartable task has no restart information available");
        }
    }

    drop(current_task);
    drop(held_interrupts);
    // ****************************************************
    // NOTE: nothing below here is guaranteed to run again!
    // ****************************************************

    scheduler::schedule();
    error!("BUG: task_cleanup_final(): task was rescheduled after being dead!");
    loop { }
}

/// Helper function to remove a task from its runqueue and drop it.
fn remove_current_task_from_runqueue(current_task: &TaskRef) {
    // Special behavior when evaluating runqueues
    #[cfg(rq_eval)] {
        // The special spillful version does nothing here, since it was already done in `internal_exit()`
        #[cfg(runqueue_spillful)] {
            // do nothing
        }
        // The regular spill-free version does brute-force removal of the task from ALL runqueues.
        #[cfg(not(runqueue_spillful))] {
            runqueue::remove_task_from_all(current_task).unwrap();
        }
    }

    // In the regular case, we do not perform task migration between cores,
    // so we can use the heuristic that the task is only on the current core's runqueue.
    #[cfg(not(rq_eval))] {
        if let Err(e) = runqueue::get_runqueue(apic::get_my_apic_id())
            .ok_or("couldn't get this core's ID or runqueue to remove exited task from it")
            .and_then(|rq| rq.write().remove_task(current_task)) 
        {
            error!("BUG: couldn't remove exited task from runqueue: {}", e);
        }
    }
}

/// Spawns an idle task on the current CPU and adds it to this CPU's runqueue.
pub fn create_idle_task() -> Result<JoinableTaskRef, &'static str> {
    let apic_id = get_my_apic_id();
    debug!("Spawning a new idle task on core {}", apic_id);

    new_task_builder(idle_task_entry, apic_id)
        .name(format!("idle_task_core_{}", apic_id))
        .idle(apic_id)
        .spawn_restartable(None)
}

/// A basic idle task that does nothing but loop endlessly.
/// 
/// Note: the current spawn API does not support spawning a task with the return type `!`,
/// so we use `()` here instead. 
#[inline(never)]
fn idle_task_entry(_apic_id: u8) {
    info!("Entered idle task loop on core {}: {:?}", apic::get_my_apic_id(), task::get_my_current_task());
    loop {
        // TODO: put this core into a low-power state
        pause::spin_loop_hint();
    }
}

