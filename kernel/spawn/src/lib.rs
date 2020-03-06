#![no_std]
#![feature(asm)]
#![feature(stmt_expr_attributes)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate debugit;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate memory;
extern crate kernel_config;
extern crate task;
extern crate runqueue;
extern crate scheduler;
extern crate mod_mgmt;
extern crate gdt;
extern crate owning_ref;
extern crate apic;
extern crate context_switch;
extern crate path;
extern crate fs_node;
extern crate type_name;
extern crate catch_unwind;


use core::{
    mem,
    marker::PhantomData,
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
use fs_node::FileOrDir;

#[cfg(simd_personality)]
use task::SimdExt;


/// Initializes tasking for the given AP core, including creating a runqueue for it
/// and creating its initial idle task for that core. 
pub fn init(kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>, apic_id: u8,
            stack_bottom: VirtualAddress, stack_top: VirtualAddress) 
            -> Result<TaskRef, &'static str> 
{
    runqueue::init(apic_id)?;
    
    let task_ref = task::create_idle_task(apic_id, stack_bottom, stack_top, kernel_mmi_ref)?;
    runqueue::add_task_to_specific_runqueue(apic_id, task_ref.clone())?;
    Ok(task_ref)
}


/// The argument type accepted by the `main` function entry point into each application.
type MainFuncArg = Vec<String>;

/// The type returned by the `main` function entry point of each application.
type MainFuncRet = isize;

/// The function signature of the `main` function that every application must have,
/// as it is the entry point into each application `Task`.
type MainFunc = fn(MainFuncArg) -> MainFuncRet;

/// A struct that uses the Builder pattern to create and customize new kernel `Task`s.
/// Note that the new `Task` will not actually be created until the [`spawn`](#method.spawn) method is invoked.
pub struct KernelTaskBuilder<F, A, R> {
    func: F,
    argument: A,
    _rettype: PhantomData<R>,
    name: Option<String>,
    pin_on_core: Option<u8>,
    blocked: bool,

    #[cfg(simd_personality)]
    simd: SimdExt,
}

impl<F, A, R> KernelTaskBuilder<F, A, R> 
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R,
{
    /// Creates a new `Task` from the given kernel function `func`
    /// that will be passed the argument `arg` when spawned. 
    pub fn new(func: F, argument: A) -> KernelTaskBuilder<F, A, R> {
        KernelTaskBuilder {
            argument: argument,
            func: func,
            _rettype: PhantomData,
            name: None,
            pin_on_core: None,
            blocked: false,

            #[cfg(simd_personality)]
            simd: SimdExt::None,
        }
    }

    /// Set the String name for the new Task.
    pub fn name(mut self, name: String) -> KernelTaskBuilder<F, A, R> {
        self.name = Some(name);
        self
    }

    /// Pin the new Task to a specific core.
    pub fn pin_on_core(mut self, core_apic_id: u8) -> KernelTaskBuilder<F, A, R> {
        self.pin_on_core = Some(core_apic_id);
        self
    }

    /// Mark this new Task as a SIMD-enabled Task 
    /// that can run SIMD instructions and use SIMD registers.
    #[cfg(simd_personality)]
    pub fn simd(mut self, extension: SimdExt) -> KernelTaskBuilder<F, A, R> {
        self.simd = extension;
        self
    }

    /// Set the new Task's `RunState` to be `Blocked` instead of `Runnable` when it is first spawned.
    /// This allows another task to delay the new task's execution arbitrarily, 
    /// e.g., to set up other things for the newly-spawned (but not yet running) task. 
    /// 
    /// Note that the new Task will not be `Runnable` until it is explicitly set as such.
    pub fn block(mut self) -> KernelTaskBuilder<F, A, R> {
        self.blocked = true;
        self
    }

    /// Finishes this `KernelTaskBuilder` and spawns a new kernel task in the same address space and `CrateNamespace` as the current task. 
    /// This merely makes the new task Runnable, it does not switch to it immediately; that will happen on the next scheduler invocation.
    #[inline(never)]
    pub fn spawn(self) -> Result<TaskRef, &'static str> {
        const DUMMY_PB_FUNC: Option<fn(&mut Task) -> Result<(), &'static str>> = None; // just a type-specific `None` value
        self.spawn_internal(DUMMY_PB_FUNC)
    }

    /// The internal spawn routine for both regular kernel Tasks and application Tasks.
    /// otherwise in Runnable state.
    fn spawn_internal<PB>(
        self, 
        post_builder_func: Option<PB>) 
    -> Result<TaskRef, &'static str> 
        where PB: FnOnce(&mut Task) -> Result<(), &'static str>
    {
        let mut new_task = Task::new(
            None,
            task_cleanup_failure::<F, A, R>,
        )?;
        new_task.name = self.name.unwrap_or_else(|| String::from( 
            // if a Task name wasn't provided, then just use the function's name
            type_name::get::<F>(),
        ));
    
        #[cfg(simd_personality)] {  
            new_task.simd = self.simd;
        }

        setup_context_trampoline(&mut new_task, task_wrapper::<F, A, R>);

        // set up the kthread stuff
        let kthread_call = Box::new( KthreadCall::new(self.argument, self.func) );
        // debug!("Creating kthread_call: {:?}", debugit!(kthread_call));

        // currently we're using the very bottom of the kstack for kthread arguments
        let arg_ptr = new_task.kstack.bottom().value();
        let kthread_ptr: *mut KthreadCall<F, A, R> = Box::into_raw(kthread_call);  // consumes the kthread_call Box!
        unsafe {
            *(arg_ptr as *mut _) = kthread_ptr; // as *mut KthreadCall<A, R>; // as usize;
            // debug!("checking kthread_call: arg_ptr={:#x} *arg_ptr={:#x} kthread_ptr={:#x} {:?}", arg_ptr as usize, *(arg_ptr as *const usize) as usize, kthread_ptr as usize, debugit!(*kthread_ptr));
        }
        // The new task is ready to be scheduled in, now that its stack trampoline has been set up.
        if self.blocked {
            new_task.runstate = RunState::Blocked;
        } else {
            new_task.runstate = RunState::Runnable;
        }

        // If the caller provided a post-build function, invoke that now before finalizing the task and adding it to runqueues  
        if let Some(func) = post_builder_func{
            func(&mut new_task)?;
        }

        let new_task_id = new_task.id;
        let task_ref = TaskRef::new(new_task);
        let old_task = TASKLIST.lock().insert(new_task_id, task_ref.clone());
        // insert should return None, because that means there was no existing task with the same ID 
        if old_task.is_some() {
            error!("BUG: KernelTaskBuilder::spawn(): Fatal Error: TASKLIST already contained a task with the new task's ID!");
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

/// Every executable application must have an entry point function named "main".
const ENTRY_POINT_SECTION_NAME: &'static str = "main";

/// A struct that uses the Builder pattern to create and customize new application `Task`s.
/// Note that the new `Task` will not actually be created until the [`spawn`](#method.spawn) method is invoked.
pub struct ApplicationTaskBuilder {
    path: Path,
    argument: MainFuncArg,
    name: Option<String>,
    pin_on_core: Option<u8>,
    namespace: Option<Arc<CrateNamespace>>,
    blocked: bool,

    #[cfg(simd_personality)]
    simd: SimdExt,
}

impl ApplicationTaskBuilder {
    /// Creates a new application `Task` from the given `path`, which points to 
    /// an application crate object file that must have an entry point called `main`.
    /// 
    /// TODO: change the `Path` argument to the more flexible type `IntoCrateObjectFile`.
    pub fn new(path: Path) -> ApplicationTaskBuilder {
        ApplicationTaskBuilder {
            path: path,
            argument: Vec::new(), // doesn't allocate yet
            name: None,
            pin_on_core: None,
            namespace: None,
            blocked: false,

            #[cfg(simd_personality)]
            simd: SimdExt::None,
        }
    }

    /// Set the String name for the new Task.
    pub fn name(mut self, name: String) -> ApplicationTaskBuilder {
        self.name = Some(name);
        self
    }

    /// Pin the new Task to a specific core.
    pub fn pin_on_core(mut self, core_apic_id: u8) -> ApplicationTaskBuilder {
        self.pin_on_core = Some(core_apic_id);
        self
    }

    /// Mark this new Task as a SIMD-enabled Task 
    /// that can run SIMD instructions and use SIMD registers.
    #[cfg(simd_personality)]
    pub fn simd(mut self, extension: SimdExt) -> ApplicationTaskBuilder {
        self.simd = extension;
        self
    }

    /// Set the argument strings for this Task.
    pub fn argument(mut self, argument: MainFuncArg) -> ApplicationTaskBuilder {
        self.argument = argument;
        self
    }

    /// Tells this new application Task to be spawned within and linked against the crates 
    /// in the given `namespace`. 
    /// By default, this new Task will be spawned within the same namespace as the current task.
    pub fn namespace(mut self, namespace: Arc<CrateNamespace>) -> ApplicationTaskBuilder {
        self.namespace = Some(namespace);
        self
    }

    /// Set this new application Task's `RunState` to be `Blocked` instead of `Runnable` when it is first spawned.
    /// This allows another task to delay the new task's execution arbitrarily, 
    /// e.g., to set up other things for the newly-spawned (but not yet running) task. 
    /// 
    /// Note that the new Task will not be `Runnable` until it is explicitly set as such.
    pub fn block(mut self) -> ApplicationTaskBuilder {
        self.blocked = true;
        self
    }

    /// Spawns a new application task that runs in kernel mode (currently the only way to run applications).
    /// This merely makes the new task Runnable, it does not task switch to it immediately. That will happen on the next scheduler invocation.
    /// 
    /// This is similar (but not identical) to the `exec()` system call in POSIX environments. 
    pub fn spawn(self) -> Result<TaskRef, &'static str> {
        let namespace = self.namespace.clone()
            .or_else(|| task::get_my_current_task().map(|taskref| taskref.get_namespace()))
            .ok_or("ApplicationTaskBuilder::spawn(): couldn't get current task to use its CrateNamespace")?;
        
        let crate_object_file = match (&self.path).get(namespace.dir())
            .or_else(|| Path::new(format!("{}.o", &self.path)).get(namespace.dir())) // retry with ".o" extension
        {
            Some(FileOrDir::File(f)) => f,
            _ => return Err("Couldn't find specified file path for new application crate"),
        };
        let app_crate_ref = {
            let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("couldn't get_kernel_mmi_ref")?;
            CrateNamespace::load_crate_as_application(&namespace, &crate_object_file, &kernel_mmi_ref, false)?
        };

        // Find the "main" entry point function in the new app crate
        let main_func_sec_ref = { 
            let app_crate = app_crate_ref.lock_as_ref();
            let expected_main_section_name = format!("{}{}{}", app_crate.crate_name_as_prefix(), ENTRY_POINT_SECTION_NAME, SECTION_HASH_DELIMITER);
            let main_func_sec = app_crate.find_section(|sec| {
                if sec.get_type() != SectionType::Text {
                    return false;
                }
                sec.name_without_hash() == &expected_main_section_name
            });
            main_func_sec.cloned()
        }.ok_or("ApplicationTaskBuilder::spawn(): couldn't find \"main\" function, expected function name like \"<crate_name>::main::<hash>\"\
                    --> Is this an app-level library or kernel crate? (Note: you cannot spawn a library crate with no main function)"
        )?
        .clone();

        let mut space: usize = 0; // must live as long as main_func, see MappedPages::as_func()
        let main_func = {
            let main_func_sec = main_func_sec_ref.lock();
            let mapped_pages = main_func_sec.mapped_pages.lock();
            mapped_pages.as_func::<MainFunc>(main_func_sec.mapped_pages_offset, &mut space)?
        };

        // build and spawn the actual underlying kernel Task
        let mut ktb = KernelTaskBuilder::new(*main_func, self.argument)
            .name(self.name.unwrap_or_else(|| app_crate_ref.lock_as_ref().crate_name.clone()));
        
        ktb.pin_on_core = self.pin_on_core;
        ktb.blocked = self.blocked;

        #[cfg(simd_personality)] {
            ktb.simd = self.simd;
        }

        // set up app-specific task states right before the task creation is completed
        let post_build_func = |new_task: &mut Task| -> Result<(), &'static str> {
            new_task.app_crate = Some(Arc::new(app_crate_ref));
            new_task.namespace = namespace;
            Ok(())
        };

        ktb.spawn_internal(Some(post_build_func))
    }

}


#[derive(Debug)]
struct KthreadCall<F, A, R> {
    /// comes from Box::into_raw(Box<A>)
    pub arg: *mut A,
    pub func: F,
    _rettype: PhantomData<R>,
}

impl<F, A, R> KthreadCall<F, A, R> {
    fn new(a: A, f: F) -> KthreadCall<F, A, R> where F: FnOnce(A) -> R {
        KthreadCall {
            arg: Box::into_raw(Box::new(a)),
            func: f,
            _rettype: PhantomData,
        }
    }
}



/// This function sets up the given new `Task`'s kernel stack pointer to properly jump
/// to the given entry point function when the new `Task` is first scheduled in. 
/// 
/// When a new task is first scheduled in, a `Context` struct will be popped off the stack,
/// and at the end of that struct is the address of the next instruction that will be popped off as part of the "ret" instruction, 
/// i.e., the entry point into the new task. 
/// 
/// So, this function allocates space for the saved context registers to be popped off when this task is first switched to.
/// It also sets the given `new_task`'s saved_sp (its saved stack pointer, which holds the Context for task switching).
/// 
fn setup_context_trampoline(new_task: &mut Task, entry_point_function: fn() -> !) {
    
    /// A private macro that actually creates the Context and sets it up in the `new_task`.
    /// We use a macro here so we can pass in the proper `ContextType` at runtime, 
    /// which is useful for both the simd_personality config and regular/SSE configs.
    macro_rules! set_context {
        ($ContextType:ty) => (
            let new_context_ptr = (new_task.kstack.top_usable().value() - mem::size_of::<$ContextType>()) as *mut $ContextType;
            // TODO: FIXME: use the MappedPages approach to avoid this unsafe block here
            unsafe {
                *new_context_ptr = <($ContextType)>::new(entry_point_function as usize);
                new_task.saved_sp = new_context_ptr as usize; 
            }
        );
    }


    // If `simd_personality` is enabled, all of the `context_switch*` implementation crates are simultaneously enabled,
    // in order to allow choosing one of them based on the configuration options of each Task (SIMD, regular, etc).
    // If `simd_personality` is NOT enabled, then we use the context_switch routine that matches the actual build target. 
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

    #[cfg(not(simd_personality))] {
        // The context_switch crate exposes the proper TARGET-specific `Context` type here.
        set_context!(context_switch::Context);
    }
}


/// The entry point for all new `Task`s that run in kernelspace. 
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

        // The pointer to the kthread_call struct (func and arg) was placed at the bottom of the stack when this task was spawned.
        let kthread_call_ptr: *mut KthreadCall<F, A, R> = {
            let t = curr_task_ref.lock();
            unsafe {
                // dereference it once to get the raw pointer (from the Box<KthreadCall>)
                *(t.kstack.bottom().value() as *mut *mut KthreadCall<F, A, R>) as *mut KthreadCall<F, A, R>
            }
        };

        let kthread_call: KthreadCall<F, A, R> = {
            let kthread_call_box: Box<KthreadCall<F, A, R>> = unsafe {
                Box::from_raw(kthread_call_ptr)
            };
            *kthread_call_box
        };
        let arg: A = {
            let arg_box: Box<A> = unsafe {
                Box::from_raw(kthread_call.arg)
            };
            *arg_box
        };
        let func = kthread_call.func;
        debug!("task_wrapper [1]: \"{}\" about to call kthread func {:?} with arg {:?}", curr_task_name, debugit!(func), debugit!(arg));
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
        if let Err(e) = apic::get_my_apic_id()
            .and_then(|id| runqueue::get_runqueue(id))
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
    scheduler::schedule();

    // nothing below here should ever run again, we should never ever reach this point
    error!("BUG: task_cleanup_final(): task was rescheduled after being dead!");
    loop { }
}
