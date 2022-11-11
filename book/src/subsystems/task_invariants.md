# Invariants Upheld in Task Management

Theseus enforces several key invariants related to task management in order to empower the compiler to uphold memory safety and prevent resource leaks throughout the task lifecycle.

All task lifecycle functions leverage the same set of generic type parameters. 
The trait bounds on these three type parameters `<F, A, R>` are a key aspect of task-related invariants.
* `F`: the type of the entry function, i.e., its signature including arguments and return type.
* `A`: the type of the single argument passed into the entry function `F`.
* `R`: the return type of the entry function `F`.


## Invariant 1. Spawning a new task must not violate memory safety.

Rust already ensures this for multiple concurrent userspace threads, as long as they were created using its standard library thread type.
Instead of using the standard library, Theseus provides its own task abstraction, overcoming the standard library’s need 
to extralingually accommodate unsafe, platform-specific thread interfaces, e.g. `fork()`. 
Theseus does not offer fork because it is known to be unsafe and unsuitable for SAS systems[^2], 
as it extralingually duplicates task context, states, and underlying memory regions without reflecting that aliasing at the language level.

Theseus’s task abstraction preserves safety similarly to and as an extension of Rust threads. 
The interfaces to spawn a new task (in the [spawn] crate) require specifying the exact type of the entry
function `F`, its argument `A`, and its return type `R`, with the following constraints:
1. The entry function `F` must be runnable only once, meaning it must satisfy the `FnOnce` trait bound.
2. The argument type `A` and return type `R` must be safe to transfer between threads, meaning they must satisfy the `Send` trait bound.
3. The lifetime of all three types must outlast the duration of the task itself, meaning they must have a `'static` lifetime.


All [task lifecycle management functions](./task.md#the-task-lifecycle) are fully type-safe and parameterized with the same generic type parameters, `<F,A,R>`. 
This ensures both the compile-time availability of type information and the *provenance* of that type information from head to tail (spawn to cleanup) across all stages in the task's lifecycle.
Theseus thus empowers the compiler to statically prevent invalidly-typed task entry functions with arguments, return values, or execution semantics that violate type safety or memory safety.

## Invariant 2: All task states must be released in all possible execution paths.

Releasing task states requires special consideration beyond simply dropping a Task object to prevent resource leakage, as mentioned in the previous chapter.
There are several examples of how the multiple stages of task cleanup each permit varying levels of resource release:
* The task's stack is used during unwinding and can only be cleaned up once unwinding is complete.
* The saved execution `Context` can be released when a task has exited.
* A task's runstate and exit value must persist until it has been reaped.

The task cleanup functions described in the [previous chapter](./task.md#cleaning-up-tasks) demonstrate the lengths to which Theseus goes to ensure that task states and resources are fully released in both normal and exceptional execution paths. 
In addition, as mentioned above, all cleanup functions are parameterized with the same `<F, A, R>` generic type parameters, 
which is crucial for realizing restartable tasks because the failure handler for a restartable task must know its specific type parameters for the entry function, argument, and return type in order to re-spawn a new instance of the failed task.


## Invariant 3: All memory transitively reachable from a task’s entry function must outlive that task.
Although all memory regions in Theseus are represented by `MappedPages`, which prevents use-after-free via lifetime invariants,
it is difficult to use Rust lifetimes to sufficiently express the relationship between a task and arbitrary memory regions it accesses.
The Rust language does not and cannot specify a task-based lifetime, e.g., `&'task`, to indicate that the lifetime of a given reference is tied to the lifetime of the current task.

Furthermore, a Rust program running as a task cannot specify in its code that its variables bound to objects in memory are tied to the lifetime of an underlying `MappedPages` instance, as they are hidden beneath abstractions like stacks, heaps, or static program sections (.text, .rodata, etc).
Even if possible, this would be highly unergonomic, inconvenient, and render ownership useless.
For example, all local stack variables would need to be defined as borrowed references with lifetimes derived from that of the `MappedPages` object representing the stack.

Thus, to uphold this invariant, we instead establish a clear chain of ownership: 
each task owns the `LoadedCrate` that contains its entry function,
and that `LoadedCrate` owns any other `LoadedCrate`s it depends on by means of the per-section dependencies in the crate metadata.
As such, the `MappedPages` regions containing all functions and data reachable from a task’s entry function are guaranteed
to outlive that task itself. 
This avoids the unsavory solution of littering lifetime constraints across all program variables, allowing Rust code to be written normally with the standard assumption that the
stack, heap, data, and text sections will always exist.

[^2]: Andrew Baumann, Jonathan Appavoo, Orran Krieger,
and Timothy Roscoe. *"A fork() in the road"*. In Proceedings of HotOS, 2019.

[spawn]: https://theseus-os.github.io/Theseus/doc/spawn/index.html

<!-- cspell:ignore Baumann, Appavoo, Orran, Krieger  -->
