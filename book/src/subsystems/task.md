# Tasking Subsystem in Theseus

The tasking subsystem in Theseus implements full support for [multitasking](https://en.wikipedia.org/wiki/Computer_multitasking), 
in which multiple different tasks can execute concurrently[^1]  atop a single set of shared resources, i.e., CPUs and memory.

Because Theseus is a single address space (SAS) OS, it does not have a dedicated address space for each task, and thus does not follow the classic POSIX/Unix-like "process" abstraction where a process is a group of threads that all execute the same program in the same address space.
One could consider tasks in Theseus to be the same as threads in other systems; the terms "task" and "thread" can be used interchangeably.
One could also consider the entirety of Theseus to be a single "process" in that all Tasks execute within the same address space, but the analogous similarities between a process and Theseus ends there. 


In general, the interfaces exposed by the task management subsystem in Theseus follows the Rust standard library's [model for threading](https://doc.rust-lang.org/std/thread/), with several similarities:
* You can spawn (create) a new task with a function or a closure as the entry point.
* You can customize a new task using a convenient builder pattern.
* You can wait on a task to exit by *joining* it.
* You can use any standard synchronization types for inter-task communication, e.g., shared memory or channels.
* You can catch the action of stack unwinding after a panic or exception occurs in a task.

In this way, tasks in Theseus are effectively a combination of the concept of language-level green threads and OS-level native threads.


## The `Task` struct

There is one instance of the `Task` struct for each task that currently exists in the system.
A task is often thought of as an *execution context*, and the task struct includes key information about the execution of a given program's code.
Compare to that of other OSes, the [`Task`] struct in Theseus is quite minimal in size and scope,
because our state management philosophy strives to keep only states relevant to a given subsystem in that subsystem. 
For example, scheduler-related states are not present in Theseus's task struct; rather, they are found in the relevant scheduler crate in which they are used.
In other words, Theseus's task struct is not monolithic and all-encompassing.

Theseus's task struct includes several key items:
* The actual [`Context`] of the task's on-CPU execution.
    * This holds the values of CPU registers (e.g., the stack pointer and other registers) that are saved and restored when context switching between this task and others.
* The name and unique ID of that task.
    * These are used primarily for human readability and debugging purposes.
* The runnability ([`RunState`]) of that task, i.e., whether it can be scheduled in, and its current running status.
    * This includes the task's [exit value], if it has exited.
* The *namespace* that task is running within; see [`CrateNamespace`].
    * Running tasks in different `CrateNamespace`s is one way to partially mimic the separation offered by the standard process abstraction; see [here for more info](../design/design.md#cell--crate).
* The task's [stack].
* The task's [environment], e.g., it's current working directory.
* A variety of states related to handling execution failures and the cleanup of a failed task.   
    * These are used for fault tolerance purposes like unwinding, task cleanup, and auto-restarting critical tasks upon failure. 

> Note: the source-level documentation for the [`Task`] struct describes each member field of the Task struct in much greater detail.


The Task struct itself is split into two main parts:
1. Immutable state: things that cannot change after the initial creation of the Task.
2. Mutable state: things that *can* change over the lifetime of the Task.

The immutable states are contained in the [`Task`] struct itself, while the mutable states are contained within the [`TaskInner`] struct.
Each Task struct contains a `TaskInner` instance protected by a lock.
This design serves to significantly reduce locking overhead and contention when accessing the states of a Task in a read-only manner.
Note that certain mutable states, primarily runstate information, have been moved into `Task` itself because they can be safely modified atomically, but in general, all other mutable states are placed within `TaskInner`.
We are also careful to correctly specify the public visibility of state items within the `Task` and `TaskInner` structures to ensure that they cannot be modified maliciously or accidentally by other crates.


### The `TaskRef` type

Tasks frequently need to be shared across many entities and subsystems throughout Theseus. 
To accommodate this, Theseus offers the [`TaskRef`] type, which is effectively a shared reference to a `Task`, i.e., an `Arc<Task>`. 
There are several reasons that we introduce a dedicated [`newtype`] instead of using an `Arc<Task>` directly:
* To clarify all code related to task management.
* To control the visibility (public vs. private) of items in the Task struct.
* To guarantee that the task-local data area (per-task data) is properly set up when a new Task is created.
    * A circular reference from a `Task` to its enclosing `TaskRef` is necessary to realize task-local data; see the [`TaskLocalData`] type for more details.
* To expose a limited set of functions that allow foreign crates to modify or query only certain task states, e.g., `join`ing a task.
* To *greatly* optimize comparison and equality tests between two Tasks.
    * Two `TaskRef`s are considered equal if they point to the same underlying `Task` struct. 
    * This avoids having to apply comparison tests against *all* fields in the `Task` struct, a very expensive operation.
* To prevent other entities from obtaining direct access to a `Task` struct, or wrapping a `Task` in a non-`Arc` shared pointer type.

> Note: while `TaskLocalData` offers a very basic form of per-task data, commonly known as Thread-Local Storage (TLS), full support for standard compiler-known TLS areas in the generated object code is a work in progress.

### The global task list
Like all other OSes, Theseus maintains a global list of all tasks in the system.
Currently, this [`TASKLIST`] is stored as a map from a numeric task ID to a `TaskRef`.

Tasks are added to the task list when they are initially spawned, and will remain in the task list for the entirety of their lifecycle.
It is important to note that the presence of a task in the task list is not indicative of that task's runnability or execution status.
A task is only removed from the task list once it has been *reaped*, i.e., it has completely exited and its exit value has been "taken" by another task; for example, a "parent" task may reap a "child" task that it has spawned.


## Context switching
In OS terminology, the term "context switch" is often incorrectly overloaded and casually used to refer to any number of related topics:
1. Switching from one thread to another thread in the same address space.
2. Switching from one thread to another thread in a different address space.
3. Switching from user mode to kernel mode (e.g., Ring 3 to Ring 0) during a system call.
4. Switching from a user thread to a kernel thread upon an interrupt being triggered.

Only number 1 above is what we consider to be a true context switch, and that is what we refer to here when we say "context switch." 
Number 2 above is an *address space switch*, e.g., switching page tables, a different action that could potentially occur when switching to a different task, if the next task is in a different process/address space than the current task.
Number 3 above is a *mode switch* that generally does not result in the full execution context being saved or restored; only some registers may be pushed or popped onto the stack, depending on the calling convention employed by a given platform's system calls.
Number 4 above is similar to number 3, but is trigger by the hardware rather than userspace software, so more execution context states may need to be saved/restored. 

One key aspect of context switching is that it is transparent to the actual code that is currently executing, as the lower layers of the OS kernel will save and restore execution context as needed before resuming.
Thus, context switching (with preemption) allows for multiple untrusted and uncooperative tasks to transparently share the CPU, while maintaining the idealistic model that they each are the only task executing in the entire system and have exclusive access to the CPU.

### Implementing context switching

The implementation for context switching in Theseus is split across several crates, with each crate corresponding to a particular subset of SIMD instructions being enabled.
The top-level [`context_switch`] crate automatically selects the correct version based on which subset of SIMD functionality is chosen by the target hardware platform's specification.
For example, if SSE2 was enabled, `#[cfg(target_feature = "sse2")]` would be true and the `context_switch_sse2` crate would be used as the context switching implementation.
Currently, one can select this target by using the `x86_64-theseus-sse` target while building Theseus:
```sh
make run TARGET=x86_64-theseus-sse
```

Theseus supports both SSE2 and AVX, but its default target `x86_64-theseus` disables both.
This tells the compiler to generate soft floating-point instructions instead of SIMD instructions, meaning that the SIMD register set is not used at all.
Thus, disabling SIMD results in the simplest and fastest version of context switching, as only the basic set of general-purpose CPU registers must be saved and restored; all SIMD registers can be ignored.

Context switching is inherently an unsafe operation, hence why the standard [`context_switch()`] function is marked unsafe.
It must be implemented in assembly in order to ensure that the compiler doesn't insert any instructions that modify register values in between our instructions that save/restore them.
It is only invoked by the [`task_switch()`] function, as only that function has the proper information — saved register values and the destination for the restored register values — to correctly invoke it.


## Pre-emptive vs. Cooperative Multitasking
Theseus implements full support for preemptive multitasking, in which a given task is interrupted at a certain periodic time interval in order to allow other tasks to take over execution.
This prevents one greedy task from hogging all system resources and/or starving other tasks from execution time.
In Theseus, like most other systems, we implement this using a timer interrupt that fires every few milliseconds on each CPU core.
The length of this time period is called the *timeslice*, and is a configurable setting.
Currently, this timer interrupt is set up in the [`LocalApic`] initialization routine, which runs when each CPU core is discovered and initialized.
The interrupt handler itself is very simple, and is currently found in a function called [`lapic_timer_handler()`] in `kernel/interrupts/src/lib.rs`.

Cooperative multitasking is also possible, but at the moment Theseus does not offer an easy way to disable preemption to use *only* cooperative multitasking; however, it wouldn't be difficult to add.
Currently, tasks can choose to yield the processor to other tasks by invoking [`schedule()`], which will select another task to run next and then switch to it.
This scheduling function is the same function that is invoked by the aforementioned timer interrupt handlers to preempt the current task every few milliseconds.


## The Task lifecycle

Theseus tasks follow a typical task lifecycle, which is in-part demonstrated by the possible variants of the [`RunState`] enum. 

* **Spawning**: the task is being created.
* **Running**: the task is executing.
    * A runnable task may be *blocked* to temporarily prevent it from being scheduled in.
    * A blocked task may be *unblocked* to mark it as runnable again.
    > Note: "running" and "runnable" are not the same thing.
    > * A task is considering runnable after it has spawned and before it has exited, as long as it is not blocked.
    > * A task is only said to be *running* while it is currently executing on a given CPU core, and is merely considered runnable when it is waiting to be scheduled in whilst other tasks execute.
* **Exited**: the task is no longer executing and will never execute again.
    * Completed: the task ran to completion normally and finished as expected.
    * Failed: the task stopped executing prematurely as a result of a crash (language-level panic or machine-level exception) or as a result of a kill request.


### Spawning new Tasks
The functionality to spawn (create) a new task is implemented in a separate [`spawn`] crate.
Theseus provides a `TaskBuilder` interface to allow one to customize the creation of a new task, which starts with a call to [`new_task_builder()`].
The caller must pass a function and argument when spawning a task: the function is the entry point for the task, and the argument will be passed to that entry point function.
Once the task builder has been used to suitably customize the new task, one must invoke the `spawn()` method on the `TaskBuilder` object to actually create the new `Task` instance and add it to one or more runqueues.
This does not immediately execute the task, but rather only makes it eligible to be scheduled in during the next task switch period.

The function passed into the spawn routine is actually not the first function to run when the task is first switched to.
All new tasks have the same entry point, the [`task_wrapper()`] function, which simplifies the procedure of jumping into a new task:
1. Setting up the proper stack contents
2. Invoking the task's entry function with the proper argument
3. Catching panics and exceptions during unwinding
4. Handling the task's states after it has exited, whether a completion or failure.


### Cleaning up Tasks

The `Task` struct implements Rust's `Drop` trait, meaning that once all references to a Task have ended, the Task object itself can be dropped and cleaned up.
Because Tasks are more complex than most structures, the drop handler alone isn't sufficient to properly clean up and remove all traces of that task and the effects that it has had on the system.

The clean up procedure begins after a task exits, either by running to completion or being killed upon request or after encountering a panic or exception.
* If the task ran to completion, the [`task_wrapper()`] will call [`task_cleanup_success()`], a function that marks the task as having successfully exited and stores the returned [`ExitValue`] in its `Task` struct.
* If the task did *not* finish, the [`task_wrapper()`] will call [`task_cleanup_failure()`], a function that marks the task as having encountered a failure, and stores the [reason](https://theseus-os.github.io/Theseus/doc/task/enum.KillReason.html) that it was prematurely killed in its `Task` struct.

After handling the successful or failed exit condition, the last piece of the task lifecycle is the [`task_cleanup_final()`] function, which removes the task from any runqueues it may be on, drops the final reference to that task, and then re-enables interrupts and yields the processor.
That final task reference, when dropped, triggers the aforementioned drop handler for the Task struct, which automatically releases all of its acquired states and allocated resources.
Note that the procedure of stack unwinding accomplishes the release of most resources and allocations, since those are represented by owned values that exist on the program's stack.

> Note: there are a separate set of similar task lifecycle functions for critical system tasks that were spawned as *restartable*. The primary difference is that after the cleanup actions are completed, a new task is spawned with the same entry point and initial argument as the failed task. 


<!-- End of section -->

[^1]: Multitasking on a uniprocessor machine (with a single CPU core) is not *truly* concurrent, it just appears to be concurrent over a given time interval because the execution of multiple tasks is quickly interleaved.
      True concurrency can be achieved on a multiprocessor machine, in which different tasks execute simultaneously on different cores.


<!-- Links below -->
[`Task`]: https://theseus-os.github.io/Theseus/doc/task/struct.Task.html
[`TaskInner`]: https://github.com/theseus-os/Theseus/blob/d6b86b6c46004513735079bed47ae21fc5d4b29d/kernel/task/src/lib.rs#L237
[`TaskRef`]: https://theseus-os.github.io/Theseus/doc/task/struct.TaskRef.html
[`TASKLIST`]: https://theseus-os.github.io/Theseus/doc/task/struct.TASKLIST.html
[environment]: https://theseus-os.github.io/Theseus/doc/environment/struct.Environment.html
[`schedule()`]: https://theseus-os.github.io/Theseus/doc/scheduler/fn.schedule.html
[`LocalApic`]: https://theseus-os.github.io/Theseus/doc/apic/struct.LocalApic.html
[`lapic_timer_handler()`]: https://github.com/theseus-os/Theseus/blob/d6b86b6c46004513735079bed47ae21fc5d4b29d/kernel/interrupts/src/lib.rs#L380
[`Context`]: https://theseus-os.github.io/Theseus/doc/context_switch/struct.Context.html
[`RunState`]: https://theseus-os.github.io/Theseus/doc/task/enum.RunState.html
[`ExitValue`]: https://theseus-os.github.io/Theseus/doc/task/enum.ExitValue.html
[`CrateNamespace`]: https://theseus-os.github.io/Theseus/doc/mod_mgmt/struct.CrateNamespace.html
[exit value]: https://theseus-os.github.io/Theseus/doc/task/enum.ExitValue.html
[`spawn`]: https://theseus-os.github.io/Theseus/doc/spawn/index.html
[stack]: https://theseus-os.github.io/Theseus/doc/stack/struct.Stack.html
[`newtype`]: https://doc.rust-lang.org/book/ch19-04-advanced-types.html#using-the-newtype-pattern-for-type-safety-and-abstraction
[`TaskLocalData`]: https://github.com/theseus-os/Theseus/blob/d6b86b6c46004513735079bed47ae21fc5d4b29d/kernel/task/src/lib.rs#L1085
[`context_switch`]: https://theseus-os.github.io/Theseus/doc/context_switch/index.html
[`context_switch()`]: https://theseus-os.github.io/Theseus/doc/context_switch_regular/fn.context_switch_regular.html
[`task_switch()`]: https://theseus-os.github.io/Theseus/doc/task/struct.Task.html#method.task_switch
[`new_task_builder()`]: https://theseus-os.github.io/Theseus/doc/spawn/fn.new_task_builder.html
[`task_wrapper()`]: https://github.com/theseus-os/Theseus/blob/d6b86b6c46004513735079bed47ae21fc5d4b29d/kernel/spawn/src/lib.rs#L500
[`task_cleanup_success()`]: https://github.com/theseus-os/Theseus/blob/d6b86b6c46004513735079bed47ae21fc5d4b29d/kernel/spawn/src/lib.rs#L547
[`task_cleanup_failure()`]: https://github.com/theseus-os/Theseus/blob/d6b86b6c46004513735079bed47ae21fc5d4b29d/kernel/spawn/src/lib.rs#L587
[`task_cleanup_final()`]: https://github.com/theseus-os/Theseus/blob/d6b86b6c46004513735079bed47ae21fc5d4b29d/kernel/spawn/src/lib.rs#L631
