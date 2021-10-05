# Memory Management in Theseus

Memory management is one of the most unique aspects of Theseus's design, compared to how other OSes manage memory. 

## Single *Virtual* Address Space
As previously mentioned, Theseus is a Single Address Space (SAS) OS, meaning that all kernel entities, libraries, and applications are loaded into and execute within a single address space. This is possible due to careful design combined with isolation based on Rust's type and memory safety instead of hardware-based memory protection.

That being said, Theseus's single address space is a *virtual* address space, not a physical address space; 
all Rust-level pointers that are dereferenced and addresses that are accessed are virtual addresses. 
Although Theseus could technically operate directly on physical memory addresses without using virtual memory at all, the use of virtual addresses affords us many benefits, e.g., easier contiguous memory allocation, guard pages to catch stack overflow, etc. 


## Types and Terminology
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

### `Page`s, `Frame`s, and ranges
A chunk of virtual memory is called a `Page`, while a chunk of physical memory is called a `Frame`. 
`Page`s and `Frame`s have the same size, typically 4KiB (4096 bytes) but ultimately dependent upon hardware.
These chunks are the smallest elementary unit that the hardware's Memory Management Unit (MMU) can operate on, i.e., they are indivisible from the hardware's point of view. 
In other words, the MMU hardware cannot map any chunk of memory smaller than a single `Page` to any chunk of memory smaller than a single `Frame`. 

A `Page` has a starting `VirtualAddress` and an ending `VirtualAddress`; for example, a `Page` may start (inclusively) at address `v0x5000` and end (exlusively) at `v0x6000` 
Similarly, a `Frame` has a starting `PhysicalAddress` and an ending `PhysicalAddress`, for example from `p0xFFFD1000` to `p0xFFFD2000`.
A `Page` can be said to contain a `VirtualAddress` within its bounds, and likewise a `Frame` can be said to contain a `PhysicalAddress`.
Although `Page`s and `Frame`s have internal numbers, we typically identify them by their starting address, e.g., "the page starting at `v0x9000`" instead of "page 9".
Intrinsically, `Page`s have no relation to `PhysicalAddress`es, and similarly, `Frame`s have no relation to `VirtualAddress`es.


For convenience, Theseus provides dedicated "range" types to represent a contiguous range of virtual `Page`s or physical `Frame`s. 
They are inclusive ranges on both sides; see Rust's built-in [`RangeInclusive`] type for more information.
These types implement the standard Rust [`Iterator`] trait, allowing for easy iteration over all pages or frames in a range.

Theseus employs macros to generate the implementation of the above basic types,
as they are symmetric across the virtual memory and physical memory categories.
This ensures that the `VirtualAddress` and `PhysicalAddress` have the same interface (public methods); the same is true for `Page` and`Frame`, and so on.


[`VirtualAddress`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.VirtualAddress.html
[`PhysicalAddress`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.PhysicalAddress.html
[`Page`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.Page.html
[`Frame`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.Frame.html
[`PageRange`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.PageRange.html
[`FrameRange`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.FrameRange.html
[`page_allocator`]:  https://theseus-os.github.io/Theseus/doc/page_allocator/index.html
[`frame_allocator`]: https://theseus-os.github.io/Theseus/doc/frame_allocator/index.html
[`Iterator`]: https://doc.rust-lang.org/std/iter/trait.Iterator.html
[`RangeInclusive`]: https://doc.rust-lang.org/std/ops/struct.RangeInclusive.html


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

## Advanced memory types
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

In order to actually *use* and *access* memory, you must map a virtual address to a physical address. 
You cannot access a physical address directly because the CPU expects to work with *virtual* addresses that will be auto-translated to physical addresses by the MMU and other memory controller hardware[^1].
Also, a virtual address is useless by itself; it doesn't have any content or purpose until you first map it to a real underlying physical address.

Memory mapping involves setting up a page table and establishing page table entries to represent the virtual-to-physical address mapping in a way that the MMU can understand.
As long as a mapping exists in the currently-active page table, you can access the contents in real physical memory by accessing a virtual address that is mapped to it. 
Accessing memory simply means dereferencing a pointer at a particular virtual address; a pointer is effectively a typed virtual addresses.

Attempting to access a virtual address that is not currently mapped will result in a *page fault*, a CPU exception that temporarily halts normal execution flow to allow a page fault handler in the OS to attempt to set up an appropriate page table entry for the non-mapped virtual address.
Using page faults is how on-demand paging is realized; Theseus currently does not do this because it goes against the [PHIS principle](../design/idea.md#pie-principle).


Theseus's memory subsystem specifies three key types for mapping memory:
* [`Mapper`]: provides functions to map virtual memory to physical memory, which create and return `MappedPages` objects.
* [`PageTable`]: a top-level page table, which owns the root frame of the page table and a `Mapper` instance that uses that page table frame to create new page table entries.
    * This auto-derefs into a `Mapper`, allowing all `Mapper` functions to be called on a `PageTable` object.
* [`MappedPages`]: an object that represents a range of virtually-contiguous pages that are mapped to physical frames and have a single exclusive owner. 


The `Mapper` type implements the core two functions for mapping memory in Theseus:
1. `map_allocated_pages()`: maps a specific range of `AllocatedPages` to frames that are allocated on demand by the system at a random physical address.
    * Useful if you have some virtual pages and just want to use them for general purposes, and you don't care which physical memory gets mapped to it.
2. `map_allocated_pages_to()`: maps a specific range of `AllocatedPages` to a specific range of `AllocatedFrames`.
    * Useful if you need to access a hardware device or other memory that exists at a specific physical address.


The astute reader will notice that you can *only* map a range of exclusively-owned `AllocatedPages` to a range of exclusively-owned `AllocatedFrames`.
You cannot simply map a raw virtual address or page to a raw physical address or frame, they have to be specifically requested from the corresponding allocator and then mapped using the current active page table.
This is part of Theseus's solution to ensure that accessing *any* arbitrary region of memory is guaranteed safe at compile time.  


[^1]: This assumes the CPU is in a standard mode like protected mode or long mode with paging enabled. The MMU can be disabled, after which virtual addresses do not exist and physical addresses can be directly used, but Theseus (and most other OSes) do not disable the MMU. 

[`Mapper`]: https://theseus-os.github.io/Theseus/doc/memory/struct.Mapper.html
[`PageTable`]: https://theseus-os.github.io/Theseus/doc/memory/struct.PageTable.html


### The `MappedPages` type

The `MappedPages` type represents a region of virtually-contiguous pages that are statically guaranteed to be mapped to real physical frames (which may or may not be contiguous).
A `MappedPages` instance includes the following items:
* The range of `AllocatedPages` that it maps
* The permissions flags with which it was mapped, e.g., whether it is read-only, writable, cacheable, etc.
* The root page table under which it was mapped, for extra safety and sanity checks 

`MappedPages` is the fundamental, sole way to represent and access mapped memory in Theseus; it serves as the backing representation for stacks, heaps, and other arbitrary memory regions, e.g., device MMIO and loaded cells.

#### Invariants and Safety Guarantees at Compile-time
The design of `MappedPages` empowers the compiler's type system to enforce the following key invariants, extending Rust's memory safety checks to *all* OS-known memory regions, not just the compiler-known stack and heap.
1. The mapping from virtual pages to physical frames must be one-to-one, or bijective.
2. A memory region must be unmapped exactly once, only after no outstanding references to it remain.
3. A memory region must not be accessible beyond its bounds.
4. A memory region can only be referenced as mutable or executable if mapped as such.

These invariants tie in nicely to Rust's existing memory safety rules, such as preventing multiple invalid aliases (aliasing XOR mutability), out-of-bounds access, use after free and double free, and forbidden mutable access.


#### How to use `MappedPages` 
TODO: describe the safe abstractions for reinterpreting arbitrary memory regions as a reference to a specific type.
* as_type():
* as_type_mut():
* as_slice():
* as_slice_mut():
* `LoadedSection::as_func()`: 

TODO: mention that other OSes just use raw virtual address values that you have to unsafely cast to a typed pointer of your choice. There is also no lifetime guarantees to ensure how long the mapping and its contained virtual addresses are valid for, so it's extremely easy to cause unsafe, undefined behavior by using a raw virtual address after it has been unmapped, causing segfaults.


The `MappedPages` type also exposes other convenient utility methods:
* remap(): change the permissions flags for the virtual pages, but they're still mapped to the same physical frames.
* merge(): combine multiple contiguous `MappedPages` objects into one
* unmap_into_parts(): unmap the memory region and re-take ownership of its constituent parts (`AllocatedPages`, etc) for future use.
    * Without calling this, a `MappedPages` object will be auto-unmapped and its constituent parts deallocated for future use,
      but that will happen behind the scenes without you being able to directly access them.
* You can also call any of the methods from `PageRange` as well.



### Deallocating frames
Deallocating virtual pages is easy because the range of `AllocatedPages` is directly stored in and owned by a `MappedPages` object, so it is simply a matter of deallocating them once they are dropped.

However, deallocating a range of `AllocatedFrames` is much more difficult because each page in a range of virtually-contiguous pages may likely be mapped to  necessarily be mapped to a single set of 

In existing OSes, there is no way to easily and immediately determine which frames are mapped to which virtual pages; this requires a *reverse mapping* from `1` frame to `N` pages, which is prohibitively expensive to maintain.
As such, OS kernels typically run a periodic "garbage collection" thread on idle CPUs that sweeps the page tables to determine 

However, Theseus's design vastly simplifies the procedure of reclaiming unused physical frames for deallocation.
The single address space design and guaranteed bijective (1-to-1) mappings mean that 


## Heap: dynamic memory allocation
Heaps are used to dynamically allocate chunks of memory smaller than the granularity of one page.  

TODO: describe our heap design and how we integrate that with Rust's (old) requirement of a single global allocator.

> Note: Theseus's heap design that combines an early heap, fully-featured heap, and per-core dedicated heaps all into a single heap abstraction was implemented before Rust's `alloc` types supported non-global allocators. We haven't yet investigated 

