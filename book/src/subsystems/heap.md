# Heap: dynamic memory allocation
Heaps are used to dynamically allocate chunks of memory smaller than the granularity of one page.

> Note: One can request a large allocation from the heap, but in Theseus it will be backed by an individually-created `MappedPages` object of newly-allocated pages and frames that are mapped to one another, so it's generally less efficient to use the heap for large allocations.

In Theseus, the primary purpose of the heap is to enable the usage of Rust's [`alloc`] types, e.g., `Box`, `Arc`, `Vec`, and other dynamically-allocated collections types.
Heap allocators must implement Rust's [`GlobalAlloc`] trait in order to be used as the backing allocator behind these alloc types.
 how we integrate that with Rust's (old) requirement of a single global allocator.

## Overview of Relevant Crates
* [`heap`]: the default heap implementation that offers a static singleton fixed-size block allocator.
    * This is the first heap initialized and created during early OS boot.
    * [`block_allocator`]: the underlying allocator implementation that optimizes allocations of common power-of-two sizes, e.g., 8 bytes, 32 bytes, etc.
        * Uses the [linked_list_allocator] crate as a fallback for uncommon allocation sizes.
* [`multiple_heaps`]: a more complex allocator that implements multiple heaps of arbitrary sizes and usage patterns.
    * Each internal heap instance is based on a zone allocator, which are modified versions of slab allocators from the [slabmalloc] crate. 
    * Unused heap space can easily be transferred among different internal heap instances for rapid, efficient heap growth.
    * Currently, one internal heap is created for each CPU core, with the core ID being used to identify and select which heap should be used for allocation.
    * It is trivially easy to use `multiple_heaps` in a different way, such as per-task heaps or per-namespace heaps.


One unique aspect of Theseus's "combination" heap design is that the early heap, fully-featured heap, and per-core dedicated heaps are all combined into a single heap abstraction that can be accessed via singleton global heap instance.
It starts out with the simple block allocator described above, and then once more key system functionality has been ininitialized during OS boot, the [`switch_to_multiple_heaps()`] function is invoked to transparently activate the more complex, per-core heap allocators.

Another unique aspect of heaps in Theseus is that all entities across the system use and share the same set of global heaps. This allows allocations to seamlessly flow and be passed among applications, libraries, and kernel entities without the need for inefficient and complex [exchange heaps] used in other SAS OSes. 



> Note: Theseus's combination heap design was implemented before Rust's `alloc` types supported non-global allocators and placement constructors.
> 
> We haven't yet investigated how to break these heap instances down into individual allocators that can be used with specific allocation palcement functions like [`Box::new_in()`](https://doc.rust-lang.org/std/boxed/struct.Box.html#method.new_in).
> 
> If you're interested in working on this, please file an issue on GitHub or otherwise contact the Theseus maintainers.




<!-- Links below -->
[`alloc`]: https://doc.rust-lang.org/alloc/
[`GlobalAlloc`]: https://doc.rust-lang.org/alloc/alloc/trait.GlobalAlloc.html
[`heap`]: https://theseus-os.github.io/Theseus/doc/heap/index.html
[`block_allocator`]: https://theseus-os.github.io/Theseus/doc/block_allocator/struct.FixedSizeBlockAllocator.html
[linked_list_allocator]: https://crates.io/crates/linked_list_allocator
[slabmalloc]: https://crates.io/crates/slabmalloc
[exchange heaps]: https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/EuroSys2007_SealedProcesses.pdf
[`multiple_heaps`]: https://theseus-os.github.io/Theseus/doc/multiple_heaps/index.html
[`switch_to_multiple_heaps()`]: https://theseus-os.github.io/Theseus/doc/multiple_heaps/fn.switch_to_multiple_heaps.html