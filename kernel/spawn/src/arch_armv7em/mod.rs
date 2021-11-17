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

use core::{
    mem,
    marker::PhantomData,
    ops::Deref,
};
use alloc::{
    string::String,
    boxed::Box,
};
use irq_safety::{hold_interrupts, enable_interrupts};
use stack::Stack;
use task::{Task, TaskRef, get_my_current_task, RunState, TASKLIST};
use interrupts;

const APIC_ID: u8 = 0;

/// Initializes tasking for the given AP core, including creating a runqueue for it
/// and creating its initial task bootstrapped from the current execution context for that core. 
pub fn init(
    apic_id: u8,
    stack: Stack,
) -> Result<BootstrapTaskRef, &'static str> {
    runqueue::init(apic_id)?;
    
    let task_ref = task::bootstrap_task(apic_id, stack)?;

    // if we are using a realtime scheduler, we would like to initialize the bootstrap task as an aperiodic task
    #[cfg(realtime_scheduler)]
    runqueue::add_task_to_specific_runqueue_realtime(apic_id, task_ref.clone(), None)?;
    #[cfg(not(realtime_scheduler))]
    runqueue::add_task_to_specific_runqueue(apic_id, task_ref.clone())?;
    Ok(BootstrapTaskRef {
        apic_id, 
        task_ref,
    })
}

/// A wrapper around a `TaskRef` that is for bootstrapped tasks. 
/// 
/// See `spawn::init()` and `task::bootstrap_task()`.
/// 
/// This exists such that a bootstrapped task can be marked as exited and removed
/// when being dropped.
pub struct BootstrapTaskRef {
    #[allow(dead_code)]
    apic_id: u8,
    task_ref: TaskRef,
}
impl Deref for BootstrapTaskRef {
    type Target = TaskRef;
    fn deref(&self) -> &TaskRef {
        &self.task_ref
    }
}
impl Drop for BootstrapTaskRef {
    fn drop(&mut self) {
        // trace!("Dropping Bootstrap Task on core {}: {:?}", self.apic_id, self.task_ref);
        remove_current_task_from_runqueue(&self.task_ref);
        let _res1 = self.mark_as_exited(Box::new(()));
        let _ev = self.take_exit_value();
    }
}


/// Creates a builder for a new `Task` that starts at the given entry point function `func`
/// and will be passed the given `argument`.
/// 
/// # Note 
/// The new task will not be spawned until [`TaskBuilder::spawn()`](struct.TaskBuilder.html#method.spawn) is invoked. 
/// See the `TaskBuilder` documentation for more details. 
///
#[cfg(not(realtime_scheduler))] 
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

/// Creates a builder for a new `Task` that starts at the given entry point function `func`
/// and will be passed the given `argument`.
/// 
/// # Note 
/// The new task will not be spawned until [`TaskBuilder::spawn()`](struct.TaskBuilder.html#method.spawn) is invoked. 
/// See the `TaskBuilder` documentation for more details. 
/// 
/// In the case of realtime scheduling, the period of the task must also be declared
#[cfg(realtime_scheduler)]
pub fn new_task_builder<F, A, R>(
    func: F,
    argument: A,
    period: Option<usize>
) -> TaskBuilder<F, A, R>
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R,
{
    TaskBuilder::new(func, argument, period)
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
    /// In the case of realtime scheduling, we must include the period of the task we would like to create
    #[cfg(realtime_scheduler)]
    period: Option<usize>,
}

impl<F, A, R> TaskBuilder<F, A, R> 
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R,
{
    /// Creates a new `Task` from the given function `func`
    /// that will be passed the argument `arg` when spawned. 
    #[cfg(not(realtime_scheduler))]
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
        }
    }

    /// Creates a new `Task` from the given function `func`
    /// that will be passed the argument `arg` when spawned. 
    /// Used in the case of realtime scheduling, where the period of the task must be specified as well
    #[cfg(realtime_scheduler)]
    fn new(func: F, argument: A, period: Option<usize>) -> TaskBuilder<F, A, R> {
        TaskBuilder {
            argument: argument,
            func: func,
            _return_type: PhantomData,
            name: None,
            pin_on_core: None,
            blocked: false,
            idle: false,
            post_build_function: None,
            period: period,
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
            let bottom_of_stack: &mut usize = new_task.kstack.as_type_mut(0)?;
            let box_ptr = Box::into_raw(Box::new(TaskFuncArg::<F, A, R> {
                arg:  self.argument,
                func: self.func,
                _rettype: PhantomData,
            }));
            *bottom_of_stack = box_ptr as usize;
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
            #[cfg(not(realtime_scheduler))]
            runqueue::add_task_to_specific_runqueue(core, task_ref.clone())?;
            #[cfg(realtime_scheduler)]
            runqueue::add_task_to_specific_runqueue_realtime(core, task_ref.clone(), self.period)?;
        }
        else {
            #[cfg(not(realtime_scheduler))]
            runqueue::add_task_to_any_runqueue(task_ref.clone())?;
            #[cfg(realtime_scheduler)]
            runqueue::add_task_to_any_runqueue_realtime(task_ref.clone(), self.period)?;
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
}

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

    // We write the new Context struct at the top of the stack, which is at the end of the stack's MappedPages. 
    // We subtract "size of usize" bytes to ensure the new Context struct doesn't spill over past the top of the stack.
    let ef_offset = new_task.kstack.size_in_bytes()
                    - mem::size_of::<usize>()
                    - mem::size_of::<interrupts::ExceptionFrame>();
    let ctx_offset = ef_offset - mem::size_of::<context_switch::Context>();
    let new_exception_frame_destination: &mut interrupts::ExceptionFrame = new_task.kstack.as_type_mut(ef_offset)?;
    *new_exception_frame_destination = <interrupts::ExceptionFrame>::new(entry_point_function as usize);
    let new_context_destination: &mut context_switch::Context = new_task.kstack.as_type_mut(ctx_offset)?;
    *new_context_destination = <context_switch::Context>::new(0xFFFF_FFF9 as usize);
    new_task.saved_sp = new_context_destination as *const _ as usize;

    Ok(())
}

/// Internal code of `task_wrapper` shared by `task_wrapper` and 
/// `task_wrapper_restartable`. 
fn task_wrapper_internal<F, A, R>() -> Result<R, task::KillReason>
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    // This is scoped to ensure that absolutely no resources that require dropping are held
    // when invoking the task's entry function, in order to simplify cleanup when unwinding.
    // That is, only non-droppable values on the stack are allowed, nothing can be allocated/locked.
    let (func, arg) = {
        let curr_task_ref = get_my_current_task().expect("BUG: task_wrapper: couldn't get current task (before task func).");

        // This task's function and argument were placed at the bottom of the stack when this task was spawned.
        let task_func_arg = {
            let t = curr_task_ref.lock();
            let tfa_box_raw_ptr: &usize = t.kstack.as_type(0)
                .expect("BUG: task_wrapper: couldn't access task's function/argument at bottom of stack");
            // SAFE: we placed this Box in this task's stack in the `spawn()` function when creating the TaskFuncArg struct.
            let tfa_boxed = unsafe { Box::from_raw((*tfa_box_raw_ptr) as *mut TaskFuncArg<F, A, R>) };
            *tfa_boxed // un-box it
        };
        let (func, arg) = (task_func_arg.func, task_func_arg.arg);

        #[cfg(not(any(rq_eval, downtime_eval)))]
        debug!("task_wrapper [1]: \"{}\" about to call task entry func {:?} {{{}}} with arg {:?}",
            curr_task_ref.lock().name.clone(), debugit!(func), core::any::type_name::<F>(), debugit!(arg)
        );

        (func, arg)
    };

    enable_interrupts(); // we must enable interrupts for the new task, otherwise we won't be able to preempt it.

    // Now we actually invoke the entry point function that this Task was spawned for, catching a panic if one occurs.

    // Skip catching unwind since we currently do not support it.
    // catch_unwind::catch_unwind_with_arg(func, arg)

    // Directly jump to the entry function.
    Ok(func(arg))
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


/// Internal function cleans up a task that exited properly. 
/// Contains the shared code between `task_cleanup_success` and `task_cleanup_success_restartable`
#[inline(always)]
fn task_cleanup_success_internal<R>(current_task: TaskRef, exit_value: R) -> (irq_safety::HeldInterrupts, TaskRef)
    where R: Send + 'static,
{ 
    // Disable preemption (currently just disabling interrupts altogether)
    let held_interrupts = hold_interrupts();

    #[cfg(not(rq_eval))]
    debug!("task_cleanup_success: {:?} successfully exited with return value {:?}", current_task.lock().name, debugit!(exit_value));
    if current_task.mark_as_exited(Box::new(exit_value)).is_err() {
        error!("task_cleanup_success: {:?} task could not set exit value, because task had already exited. Is this correct?", current_task.lock().name);
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



/// Internal function that clean up the task not exited properly.
#[inline(always)]
fn task_cleanup_failure_internal(current_task: TaskRef, kill_reason: task::KillReason) -> (irq_safety::HeldInterrupts, TaskRef) {
    // Disable preemption (currently just disabling interrupts altogether)
    let held_interrupts = hold_interrupts();

    #[cfg(not(downtime_eval))]
    debug!("task_cleanup_failure: {:?} panicked with {:?}", current_task.lock().name, kill_reason);

    if current_task.mark_as_killed(kill_reason).is_err() {
        error!("task_cleanup_failure: {:?} task could not set kill reason, because task had already exited. Is this correct?", current_task.lock().name);
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
// fn task_restartable_cleanup_failure<F, A, R>(current_task: TaskRef, kill_reason: task::KillReason) -> !
//     where A: Send + Clone + 'static, 
//           R: Send + 'static,
//           F: FnOnce(A) -> R + Send + Clone + 'static, 
// {
//     let (held_interrupts, current_task) = task_cleanup_failure_internal(current_task, kill_reason);
//     task_restartable_cleanup_final::<F, A, R>(held_interrupts, current_task)
// }



/// The final piece of the task cleanup logic,
/// which removes the task from its runqueue and permanently deschedules it. 
fn task_cleanup_final<F, A, R>(held_interrupts: irq_safety::HeldInterrupts, current_task: TaskRef) -> ! 
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    remove_current_task_from_runqueue(&current_task);
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
        if let Err(e) = runqueue::get_runqueue(APIC_ID)
            .ok_or("couldn't get this core's ID or runqueue to remove exited task from it")
            .and_then(|rq| rq.write().remove_task(current_task)) 
        {
            error!("BUG: couldn't remove exited task from runqueue: {}", e);
        }
    }
}

/// Spawns an idle task on the given `core` if specified, otherwise on the current core. 
/// Then, it adds adds the new idle task to that core's runqueue.
pub fn create_idle_task(core: Option<u8>) -> Result<TaskRef, &'static str> {
    let apic_id = core.unwrap_or_else(|| APIC_ID);
    debug!("Spawning a new idle task on core {}", apic_id);

    #[cfg(not(realtime_scheduler))]
    return new_task_builder(dummy_idle_task, apic_id)
        .name(format!("idle_task_core_{}", apic_id))
        .idle(apic_id)
        .spawn();

    // In the case of realtime scheduling, we would like to initialize the idle task as an aperiodic task
    #[cfg(realtime_scheduler)]
    return new_task_builder(dummy_idle_task, apic_id, None)
        .name(format!("idle_task_core_{}", apic_id))
        .idle(apic_id)
        .spawn();
}

/// Dummy `idle_task` to be used if original `idle_task` crashes.
/// 
/// Note: the current spawn API does not support spawning a task with the return type `!`,
/// so we use `()` here instead. 
#[inline(never)]
fn dummy_idle_task(_apic_id: u8) {
    info!("Entered idle task loop on core {}: {:?}", _apic_id, task::get_my_current_task());
    loop {
        // TODO: put this core into a low-power state
        pause::spin_loop_hint();
    }
}

