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
extern crate task;
extern crate runqueue;
extern crate scheduler;
extern crate mod_mgmt;
extern crate apic;
extern crate context_switch;
extern crate path;
extern crate fs_node;
extern crate catch_unwind;


use core::{
    mem,
    marker::PhantomData,
    any::Any,
    sync::atomic::{Ordering, AtomicBool, compiler_fence},
};
use alloc::{
    vec::Vec,
    string::String,
    sync::Arc,
    boxed::Box,
};
use irq_safety::{MutexIrqSafe, hold_interrupts, enable_interrupts};
use memory::{get_kernel_mmi_ref, MemoryManagementInfo, VirtualAddress};
use task::{Task, TaskRef, get_my_current_task, RunState, TASKLIST};
use mod_mgmt::{CrateNamespace, SectionType, SECTION_HASH_DELIMITER};
use path::Path;
use apic::get_my_apic_id;
use fs_node::FileOrDir;

#[cfg(simd_personality)]
use task::SimdExt;


/// Initializes tasking for the given AP core, including creating a runqueue for it
/// and creating its initial idle task for that core. 
pub fn init(
    kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>,
    apic_id: u8,
    stack_bottom: VirtualAddress,
    stack_top: VirtualAddress
) -> Result<TaskRef, &'static str> {
    runqueue::init(apic_id)?;
    
    let task_ref = task::create_idle_task(apic_id, stack_bottom, stack_top, kernel_mmi_ref)?;
    runqueue::add_task_to_specific_runqueue(apic_id, task_ref.clone())?;
    Ok(task_ref)
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
    
    let namespace = new_namespace.clone()
        .or_else(|| task::get_my_current_task().map(|taskref| taskref.get_namespace()))
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

    let mut space: usize = 0; // must live as long as main_func, see MappedPages::as_func()
    let main_func = {
        let mapped_pages = main_func_sec.mapped_pages.lock();
        mapped_pages.as_func::<MainFunc>(main_func_sec.mapped_pages_offset, &mut space)?
    };

    // Create the underlying task builder. 
    // Give it a default name based on the app crate's name, but that can be changed later. 
    let mut tb = TaskBuilder::new(*main_func, MainFuncArg::default())
        .name(app_crate_ref.lock_as_ref().crate_name.clone()); 

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

    #[cfg(simd_personality)]
    simd: SimdExt,
    post_build_function: Option<Box< dyn FnOnce(&mut Task) -> Result<(), &'static str> >>,
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

    /// Set the new Task's is_an_idle_task to true.
    /// This allows the creation of an idle task using Taskbuilder
    pub fn set_idle(mut self) -> TaskBuilder<F, A, R> {
        self.idle = true;
        self
    }

    /// Finishes this `TaskBuilder` and spawns the new task as described by its builder functions.
    /// 
    /// This merely makes the new task Runnable, it does not switch to it immediately; that will happen on the next scheduler invocation.
    #[inline(never)]
    pub fn spawn(self) -> Result<TaskRef, &'static str> {
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
        {
            let bottom_of_stack = new_task.kstack.as_type_mut::<*mut TaskFuncArg<F, A, R>>(0)?;
            *bottom_of_stack = Box::into_raw(Box::new(TaskFuncArg::<F, A, R> {
                arg:  self.argument,
                func: self.func,
                _rettype: PhantomData,
            }));
        }

        // The new task is ready to be scheduled in, now that its stack trampoline has been set up.
        if self.blocked {
            new_task.runstate = RunState::Blocked;
        } else {
            new_task.runstate = RunState::Runnable;
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
        let old_task = TASKLIST.lock().insert(new_task_id, task_ref.clone());
        // insert should return None, because that means there was no existing task with the same ID 
        if old_task.is_some() {
            error!("BUG: TaskBuilder::spawn(): Fatal Error: TASKLIST already contained a task with the new task's ID!");
            return Err("BUG: TASKLIST a contained a task with the new task's ID");
        }
        
        if let Some(core) = self.pin_on_core {
            runqueue::add_task_to_specific_runqueue(core, task_ref.clone())?;
        }
        else {
            runqueue::add_task_to_any_runqueue(task_ref.clone())?;
        }

        Ok(task_ref)
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

    /// Finishes this `TaskBuilder` and spawns the new task in a restartable way as described by its builder functions.
    /// 
    /// This merely makes the new task Runnable, it does not switch to it immediately; that will happen on the next scheduler invocation.
    #[inline(never)]
    pub fn restartable_spawn(self) -> Result<TaskRef, &'static str> {
        let mut new_task = Task::new(
            None,
            task_cleanup_failure::<F, A, R>,
        )?;
        // If a Task name wasn't provided, then just use the function's name.
        new_task.name = self.name.unwrap_or_else(|| String::from(core::any::type_name::<F>()));
    
        #[cfg(simd_personality)] {  
            new_task.simd = self.simd;
        }

        // store function and argument in the task
        let boxed_argument = Box::new(self.argument.clone());
        new_task.restart_info.argument = Some(boxed_argument);
        let boxed_func = Box::new(self.func.clone());
        new_task.restart_info.func = Some(boxed_func);

        // mark the task as restartable
        new_task.restart_info.restartable = true;

        setup_context_trampoline(&mut new_task, task_wrapper_restartable::<F, A, R>)?;

        // Currently we're using the very bottom of the kstack for kthread arguments. 
        // This is probably stupid (it'd be best to put them directly where they need to go towards the top of the stack),
        // but it simplifies type safety in the `task_wrapper` entry point and removes uncertainty from assumed calling conventions.
        {
            let bottom_of_stack = new_task.kstack.as_type_mut::<*mut TaskFuncArg<F, A, R>>(0)?;
            *bottom_of_stack = Box::into_raw(Box::new(TaskFuncArg::<F, A, R> {
                arg:  self.argument,
                func: self.func,
                _rettype: PhantomData,
            }));
        }

        // The new task is ready to be scheduled in, now that its stack trampoline has been set up.
        if self.blocked {
            new_task.runstate = RunState::Blocked;
        } else {
            new_task.runstate = RunState::Runnable;
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
        let old_task = TASKLIST.lock().insert(new_task_id, task_ref.clone());
        // insert should return None, because that means there was no existing task with the same ID 
        if old_task.is_some() {
            error!("BUG: TaskBuilder::spawn(): Fatal Error: TASKLIST already contained a task with the new task's ID!");
            return Err("BUG: TASKLIST a contained a task with the new task's ID");
        }
        
        if let Some(core) = self.pin_on_core {
            runqueue::add_task_to_specific_runqueue(core, task_ref.clone())?;
        }
        else {
            runqueue::add_task_to_any_runqueue(task_ref.clone())?;
        }

        Ok(task_ref)
    }
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
            let mp_offset = new_task.kstack.size_in_bytes() - mem::size_of::<usize>() - mem::size_of::<$ContextType>();
            let new_context_destination: &mut $ContextType = new_task.kstack.as_type_mut(mp_offset)?;
            *new_context_destination = <($ContextType)>::new(entry_point_function as usize);
            new_task.saved_sp = new_context_destination as *const _ as usize; 
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

/// The entry point for all new `Task`s except restartable tasks. 
/// This does not return, because it doesn't really have anywhere to return.
fn task_wrapper<F, A, R>() -> !
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    // This is scoped to ensure that absolutely no resources that require dropping are held
    // when invoking the task's entry function, in order to simplify cleanup when unwinding.
    // That is, only non-droppable values on the stack are allowed, nothing can be allocated/locked.
    let (func, arg) = {
        let curr_task_ref = get_my_current_task().expect("BUG: task_wrapper: couldn't get current task (before task func).");
        let curr_task_name = curr_task_ref.lock().name.clone();

        // This task's function and argument were placed at the bottom of the stack when this task was spawned.
        let task_func_arg = {
            let t = curr_task_ref.lock();
            let tfa_box_raw_ptr = t.kstack.as_type::<*mut TaskFuncArg<F, A, R>>(0)
                .expect("BUG: task_wrapper: couldn't access task's function/argument at bottom of stack");
            // SAFE: we placed this Box in this task's stack in the `spawn()` function when creating the TaskFuncArg struct.
            let tfa_boxed = unsafe { Box::from_raw(*tfa_box_raw_ptr) };
            *tfa_boxed // un-box it
        };
        let (func, arg) = (task_func_arg.func, task_func_arg.arg);
        debug!("task_wrapper [1]: \"{}\" about to call task entry func {:?} {{{}}} with arg {:?}",
            curr_task_name, debugit!(func), core::any::type_name::<F>(), debugit!(arg)
        );
        (func, arg)
    };

    enable_interrupts(); // we must enable interrupts for the new task, otherwise we won't be able to preempt it.

    // Now we actually invoke the entry point function that this Task was spawned for, catching a panic if one occurs.
    let result = catch_unwind::catch_unwind_with_arg(func, arg);

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
/// // We cannot use `task_wrapper` as it is not bounded by clone trait.
fn task_wrapper_restartable<F, A, R>() -> !
    where A: Send + Clone + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R + Send + Clone +'static,
{
    // This is scoped to ensure that absolutely no resources that require dropping are held
    // when invoking the task's entry function, in order to simplify cleanup when unwinding.
    // That is, only non-droppable values on the stack are allowed, nothing can be allocated/locked.
    let (func, arg) = {
        let curr_task_ref = get_my_current_task().expect("BUG: task_wrapper: couldn't get current task (before task func).");
        let curr_task_name = curr_task_ref.lock().name.clone();

        // This task's function and argument were placed at the bottom of the stack when this task was spawned.
        let task_func_arg = {
            let t = curr_task_ref.lock();
            let tfa_box_raw_ptr = t.kstack.as_type::<*mut TaskFuncArg<F, A, R>>(0)
                .expect("BUG: task_wrapper: couldn't access task's function/argument at bottom of stack");
            // SAFE: we placed this Box in this task's stack in the `spawn()` function when creating the TaskFuncArg struct.
            let tfa_boxed = unsafe { Box::from_raw(*tfa_box_raw_ptr) };
            *tfa_boxed // un-box it
        };
        let (func, arg) = (task_func_arg.func, task_func_arg.arg);
        debug!("task_wrapper [1]: \"{}\" about to call task entry func {:?} {{{}}} with arg {:?}",
            curr_task_name, debugit!(func), core::any::type_name::<F>(), debugit!(arg)
        );
        (func, arg)
    };

    
    enable_interrupts(); // we must enable interrupts for the new task, otherwise we won't be able to preempt it.

    let result = catch_unwind::catch_unwind_with_arg(func, arg);

    // Here: now that the task is finished running, we must clean in up by doing four things:
    // 1. Put the task into a non-runnable mode (exited or killed), and set its exit value
    // 2. Remove it from its runqueue
    // 3. If the task is restarable create a new task with same function and arguments
    // 4. Yield the CPU
    // The first three need to be done "atomically" (without interruption), so we must disable preemption before step 1.
    // Otherwise, this task could be marked as `Exited`, and then a context switch could occur to another task,
    // which would prevent this task from ever running again, so it would never get to remove itself from the runqueue.
    // Operations 1 happen in `task__restartable_cleanup_success` or `task_restartable_cleanup_failure`, 
    // while operations 2 to 4 then happen in `task_restartable_cleanup_final`.
    let curr_task = get_my_current_task().expect("BUG: task_wrapper: couldn't get current task (after task func).").clone();
    match result {
        Ok(exit_value)   => task_restartable_cleanup_success::<F, A, R>(curr_task, exit_value),
        Err(kill_reason) => task_restartable_cleanup_failure::<F, A, R>(curr_task, kill_reason),
    }
}


/// This function cleans up a task that exited properly.
fn task_cleanup_success<F, A, R>(current_task: TaskRef, exit_value: R) -> !
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R,  
{
    // Disable preemption (currently just disabling interrupts altogether)
    let held_interrupts = hold_interrupts();

    debug!("task_cleanup_success: {:?} successfully exited with return value {:?}", current_task.lock().name, debugit!(exit_value));
    if current_task.mark_as_exited(Box::new(exit_value)).is_err() {
        error!("task_cleanup_success: {:?} task could not set exit value, because task had already exited. Is this correct?", current_task.lock().name);
    }

    task_cleanup_final::<F, A, R>(held_interrupts, current_task)
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
    // Disable preemption (currently just disabling interrupts altogether)
    let held_interrupts = hold_interrupts();

    debug!("task_cleanup_failure: {:?} panicked with {:?}", current_task.lock().name, kill_reason);
    if current_task.mark_as_killed(kill_reason).is_err() {
        error!("task_cleanup_failure: {:?} task could not set kill reason, because task had already exited. Is this correct?", current_task.lock().name);
    }

    task_cleanup_final::<F, A, R>(held_interrupts, current_task)
}


/// The final piece of the task cleanup logic,
/// which removes the task from its runqueue and permanently deschedules it. 
fn task_cleanup_final<F, A, R>(_held_interrupts: irq_safety::HeldInterrupts, current_task: TaskRef) -> ! 
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    // Remove the task from its runqueue
    #[cfg(not(runqueue_state_spill_evaluation))]  // the normal case
    {
        if let Err(e) = runqueue::get_runqueue(apic::get_my_apic_id())
            .ok_or("couldn't get this core's ID or runqueue to remove exited task from it")
            .and_then(|rq| rq.write().remove_task(&current_task)) 
        {
            error!("BUG: task_cleanup_final(): couldn't remove exited task from runqueue: {}", e);
        }
    }

    // We must drop any local stack variables here since this function will not return
    drop(current_task);
    drop(_held_interrupts); // reenables preemption (interrupts)

    // Yield the CPU
    let success = scheduler::schedule();
    // Yielding will be succesful if there is atleast one task to schedule to. Which is true
    // in most cases as at least the idle task will be there. However on rare instances where 
    // the idle task has crashed this will fail. If so we spawn a new idle task.
    if !success {
        
        let apic_id = get_my_apic_id();

        debug!("Idle task not found on core{}",apic_id);

        let idle_taskref = new_task_builder(idle_task ,0)
			.name(String::from(format!("idle_task_ap{}", apic_id)))
			.pin_on_core(apic_id)
            .set_idle()
			.spawn().expect("failed to initiate idle task");
       
    }
    scheduler::schedule();

    // nothing below here should ever run again, we should never ever reach this point
    error!("BUG: task_cleanup_final(): task was rescheduled after being dead!");
    loop { }
}

/// Similar to `task_cleanup_success` but used on restartable_tasks
fn task_restartable_cleanup_success<F, A, R>(current_task: TaskRef, exit_value: R) -> !
    where A: Send + Clone + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R + Send + Clone +'static,
{
    // Disable preemption (currently just disabling interrupts altogether)
    let held_interrupts = hold_interrupts();

    debug!("task_cleanup_success: {:?} successfully exited with return value {:?}", current_task.lock().name, debugit!(exit_value));
    if current_task.mark_as_exited(Box::new(exit_value)).is_err() {
        error!("task_cleanup_success: {:?} task could not set exit value, because task had already exited. Is this correct?", current_task.lock().name);
    }

    task_restartable_cleanup_final::<F, A, R>(held_interrupts, current_task)
}

/// Similar to `task_cleanup_failure` but used on restartable_tasks
fn task_restartable_cleanup_failure<F, A, R>(current_task: TaskRef, kill_reason: task::KillReason) -> !
    where A: Send + Clone + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R + Send + Clone +'static, 
{
    // Disable preemption (currently just disabling interrupts altogether)
    let held_interrupts = hold_interrupts();

    debug!("task_cleanup_failure: {:?} panicked with {:?}", current_task.lock().name, kill_reason);
    if current_task.mark_as_killed(kill_reason).is_err() {
        error!("task_cleanup_failure: {:?} task could not set kill reason, because task had already exited. Is this correct?", current_task.lock().name);
    }

    task_restartable_cleanup_final::<F, A, R>(held_interrupts, current_task)
}

/// The final piece of the task cleanup logic for restartable tasks.
/// which removes the task from its runqueue and spawns it again with 
/// same entry function (F) and argument (A). 
fn task_restartable_cleanup_final<F, A, R>(_held_interrupts: irq_safety::HeldInterrupts, current_task: TaskRef) -> ! 
   where A: Send + Clone + 'static, 
         R: Send + 'static,
         F: FnOnce(A) -> R + Send + Clone +'static, 
{

    { 
        let curr_task_name = current_task.lock().name.clone();

        // (2) Remove it from its runqueue
        #[cfg(not(runqueue_state_spill_evaluation))]  // the normal case
        {
            let id = apic::get_my_apic_id();
            if let Err(e) = runqueue::get_runqueue(id)
                .ok_or("couldn't get runqueue to remove exited task from it")
                .and_then(|rq| rq.write().remove_task(&current_task)) 
            {
                error!("BUG: task_wrapper(): couldn't remove exited task from runqueue: {}", e);
            }
        }

        // (3) Restart the task if it is restartable
        if current_task.is_restartable() {
            let boxed_arg = current_task.get_argument().unwrap_or(Box::new(7));
            let boxed_arg_any = boxed_arg as Box<dyn Any>;
            let concrete_arg_ref: &A = boxed_arg_any.downcast_ref().expect("failed to downcast saved_arg into type A");

            let boxed_func = current_task.get_func().unwrap_or(Box::new(ie_loop));
            let boxed_func_any = boxed_func as Box<Any>;
            let concrete_func_ref: &(F) = boxed_func_any.downcast_ref().expect("failed to downcast saved_func into called func");

            fn ie_loop(arg: usize) {
                debug!("Hi, i'm in task_func restart with arg {}", arg);
            }
            
            TaskBuilder::new(concrete_func_ref.clone(), concrete_arg_ref.clone())
                            .name(curr_task_name)
                            .restartable_spawn()
                            .expect("Could not restart the task"); 
        }
    }

    // We must drop any local stack variables here since this function will not return
    drop(current_task);
    drop(_held_interrupts); // reenables preemption (interrupts)

    // Yield the CPU
    let success = scheduler::schedule();
    // Yielding will be succesful if there is atleast one task to schedule to. Which is true
    // in most cases as at least the idle task will be there. However on rare instances where 
    // the idle task has crashed this will fail. If so we spawn a new idle task.
    if !success {
        
        let apic_id = get_my_apic_id();

        debug!("Idle task not found on core{}",apic_id);

        let idle_taskref = new_task_builder(idle_task ,0)
			.name(String::from(format!("idle_task_ap{}", apic_id)))
			.pin_on_core(apic_id)
            .set_idle()
			.spawn().expect("failed to initiate idle task");
       
    }
    scheduler::schedule();

    // nothing below here should ever run again, we should never ever reach this point
    error!("BUG: task_cleanup_final(): task was rescheduled after being dead!");
    loop { }
}

/// Simple `idle_task` to be used if original `idle_task` crashes.
fn idle_task(_a: u32)->(){
    loop {}
}