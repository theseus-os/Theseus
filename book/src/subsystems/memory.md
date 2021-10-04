# Memory Management in Theseus

Memory management is one of the most unique aspects of Theseus's design, compared to how other OSes manage memory. 

## Single *Virtual* Address Space
As previously mentioned, Theseus is a Single Address Space (SAS) OS, meaning that all kernel entities, libraries, and applications are loaded into and execute within a single address space. This is possible due to careful design combined with isolation based on Rust's type and memory safety instead of hardware-based memory protection.

That being said, Theseus's single address space is a *virtual* address space, not a physical address space; 
all Rust-level pointers that are dereferenced and addresses that are accessed are virtual addresses. 
Although Theseus could technically operate directly on physical memory addresses without using virtual memory at all, the use of virtual addresses affords us many benefits, e.g., easier contiguous memory allocation, guard pages to catch stack overflow, etc. 


## Virtual vs. Physical Memory
Theseus uses precise, specific terminology and dedicated types to avoid confusion and correctness errors related to mixing up physical and virtual memory.
The following table concisely describes the basic memory types with links to their source-level documentation:


| Description of Type          | Virtual Memory Type   | Physical Memory Type |
|------------------------------|-----------------------|----------------------|
| A memory address             | [`VirtualAddress`]    | [`PhysicalAddress`]  |
| A chunk of memory            | [`Page`]              | [`Frame`]            |
| A range of contiguous chunks | [`PageRange`]         | [`FrameRange`]       |
| Allocator for memory chunks  | [`page_allocator`]    | [`frame_allocator`]  |

### Addresses
In Theseus, virtual and physical addresses are given dedicated, separate types that are not interoperable. 
This is to ensure that programmers are intentional about which type of address they are using and cannot accidentally mix them up.
The constructors for `VirtualAddress` and `PhysicalAddress` also ensure that you cannot create an invalid address and that all addresses used across the system are canonical in form, which is based on the hardware architecture's expectations.

For 64-bit architectures, the set of possible `VirtualAddress`es goes from `0` to `0xFFFFFFFFFFFFFFFF`, and all canonical addresses in that range can be used.
However, while the set of possible `PhysicalAddress` has the same range, there are large "holes" across the physical address space that correspond to unusable physical addresses, depending on hardware. 
Thus, you can be guaranteed that every canonical virtual address actually exists and is usable, but not every canonical physical address.

### `Page`s and `Frame`s
A chunk of virtual memory is called a `Page`, while a chunk of physical memory is called a `Frame`. 
`Page`s and `Frame`s have the same size, typically 4KiB (4096 bytes) but ultimately dependent upon hardware.
These chunks are the smallest elementary unit that the hardware's Memory Management Unit (MMU) can operate on, i.e., they are indivisible from the hardware's point of view. 
In other words, the MMU hardware cannot map any chunk of memory smaller than a single `Page` to any chunk of memory smaller than a single `Frame`. 

A `Page` has a starting `VirtualAddress` and an ending `VirtualAddress`; for example, a `Page` may start (inclusively) at address `v0x5000` and end (exlusively) at `v0x6000` 
Similarly, a `Frame` has a starting `PhysicalAddress` and an ending `PhysicalAddress`, for example from `p0xFFFD1000` to `p0xFFFD2000`.
A `Page` can be said to contain a `VirtualAddress` within its bounds, and likewise a `Frame` can be said to contain a `PhysicalAddress`.
Intrinsically, `Page`s have no relation to `PhysicalAddress`es, and similarly, `Frame`s have no relation to `VirtualAddress`es.

Although `Page`s and `Frame`s have inner numbers, we typically identify them by their starting address, e.g., "the page starting at `v0x9000`" instead of "page 9".

For convenience, Theseus provides dedicated "range" types to represent a contiguous range of virtual `Page`s or physical `Frame`s. 
They are inclusive ranges on both sides; see Rust's built-in [RangeInclusive] type for more information.
These types implement the standard Rust [Iterator] trait, allowing for easy iteration over all pages or frames in a range.

Theseus employs macros to generate the implementation of the above basic types,
as they are symmetric across the virtual memory and physical memory categories.
This ensures that the `VirtualAddress` and `PhysicalAddress` have the same interface (public methods); the same is true for `Page` and`Frame`, and so on.


[`VirtualAddress`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.VirtualAddress.html
[`PhysicalAddress`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.PhysicalAddress.html
[`Page`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.Page.html
[`Frame`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.Frame.html
[`PageRange`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.PageRange.html
[`FrameRange`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.FrameRange.html
[Iterator]: https://doc.rust-lang.org/std/iter/trait.Iterator.html
[`page_allocator`]:  https://theseus-os.github.io/Theseus/doc/page_allocator/index.html
[`frame_allocator`]: https://theseus-os.github.io/Theseus/doc/frame_allocator/index.html


### Page and Frame allocators
Theseus's [page allocator] and [frame allocator] are effectively identical in their design and implementation, but the former allocates virtual memory `Page`s while the latter allocates physical memory `Frame`s.

While the underlying implementation may change over time, the general interface for both allocators is the same. 
* You can request a new allocation of one or more `Page`s or `Frame`s at any address, which will only fail if there is no remaining virtual or physical memory. 
* You can request that allocation to start at the `Page` or `Frame` containing a specific address, but it will fail if the `Page` or `Frame` containing that address has already been allocated.
* You can also specify the minimum number of bytes that the new allocation must cover, which will be rounded up to the nearest `Page` or `Frame` granularity.
    * These allocators cannot allocate anything smaller than a single `Page` or `Frame`; for smaller allocations, you would want to use dynamically-allocated heap memory.
* Both allocators support the concept of "reserved" regions of memory, which are only usable by specific kernel entities, e.g., memory handling functions that run at early boot/init time.
    * The `frame_allocator` uses reserved regions to preserve some ranges of physical memory for specific usage, e.g., memory that contains early boot information from the bootloader, or memory that contains the actual executable kernel code.
* Both allocators also support early memory allocation before the heap is set up, allowing Theseus to bootstrap its dynamic memory management system with a series of small statically-tracked allocations in a static array.


The page allocator and frame allocator in Theseus do not directly allow you to *access* memory; you still must *map* the virtual `Page`(s) to the physical `Frame`(s) in order to access the memory therein.
We use more dedicated types to represent this, described below.

In Theseus, like other OSes, there is a single frame allocator because there is only one set of physical memory -- the actual system RAM connected to your computer's motherboard. 
It would be logically invalid and unsound to have multiple frame allocators that could independently allocate chunks of physical memory from the same single physical address space. 
Unlike other multi-address space OSes, Theseus also has a single page allocator, because we only have one virtual address space. 
All pages must be allocated from that one space of virtual addresses, therefore only a single page allocator is needed.

[page allocator]:  https://theseus-os.github.io/Theseus/doc/page_allocator/index.html
[frame allocator]: https://theseus-os.github.io/Theseus/doc/frame_allocator/index.html

### Advanced memory types
While the above "basic" types focus on preventing simple programmer mistakes through type-enforced clarity,
the below "advanced" types strive to prevent much more complex errors through type-specific invariants. 

* [`AllocatedPages`]: a range of `Page`s contiguous in virtual memory that have a single exclusive owner.
* [`AllocatedFrames`]: a range of `Frame`s contiguous in physical memory that have a single exclusive owner. 
* [`MappedPages`]: a range of virtually-contiguous pages that are mapped to physical frames and have a single exclusive owner. 
    * We will discuss `MappedPages` in a later section about memory mapping.


The only way to obtain an instance of `AllocatedPages` is to request a new allocation from the `page_allocator`; likewise, the same is true for `AllocatedFrames` and the `frame_allocator`.
Thus, the main difference between the advanced types and the basic types is that the advanced types guarantee *exclusivity*.
As such, if you have a `PageRange` from `v0x6000` to `v0x8000`, you are guaranteed that no other entity across the entire system has access to the page containing `v0x7000`.
That is a powerful guarantee that allows us to build stronger isolation and safety invariants when allocating, mapping, and accessing memory.


[`AllocatedFrames`]: https://theseus-os.github.io/Theseus/doc/frame_allocator/struct.AllocatedFrames.html
[`AllocatedPages`]: https://theseus-os.github.io/Theseus/doc/page_allocator/struct.AllocatedPages.html
[`MappedPages`]: https://theseus-os.github.io/Theseus/doc/memory/struct.MappedPages.html



## Mapping virtual memory to physical memory
Page table abstraction plus mapper abstraction. 

### The `MappedPages` type

TODO: MappedPages is the fundamental type to use for accessing and managing regions of memory. It is the backing type behind/beneath ALL other memory objects in Theseus, including stacks, heaps, device memory, MMIO regions, etc.

TODO: describe invariants and intralingual implementation. 



## Heap: dynamic memory allocation
Heaps are used to dynamically allocate chunks of memory that are (much) smaller than the granularity of one page.  

