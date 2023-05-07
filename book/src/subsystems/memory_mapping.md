# Mapping virtual memory to physical memory

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
    * This auto-dereferences into a `Mapper`, allowing all `Mapper` functions to be called on a `PageTable` object.
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


## The `MappedPages` type

The [`MappedPages`] type represents a region of virtually-contiguous pages that are *statically guaranteed* to be mapped to real physical frames (which may or may not be contiguous).
A `MappedPages` instance includes the following items:
* The range of `AllocatedPages` that it owns and are currently mapped to 
* The permissions flags with which it was mapped, e.g., whether it is read-only, writable, cacheable, etc.
* The root page table under which it was mapped, for extra safety and sanity checks.

`MappedPages` is the fundamental, sole way to represent and access mapped memory in Theseus; it serves as the backing representation for [stacks], heaps, and other arbitrary memory regions, e.g., device MMIO and loaded cells.

### Invariants and Safety Guarantees at Compile-time
The design of `MappedPages` empowers the compiler's type system to enforce the following key invariants, extending Rust's memory safety checks to *all* OS-known memory regions, not just the compiler-known stack and heap.
1. The mapping from virtual pages to physical frames must be one-to-one, or bijective.
    * Each `Page` can only be mapped to one `Frame`, and each `Frame` can only be mapped by a single `Page`, system-wide.
2. A memory region must be unmapped exactly once, only after no outstanding references to it remain.
3. A memory region must not be accessible beyond its bounds.
4. A memory region can only be referenced as mutable or executable if mapped as such.

These invariants integrate nicely with Rust's existing memory safety rules, such as preventing multiple invalid aliases (aliasing XOR mutability), out-of-bounds access, use after free and double free, and forbidden mutable access.

### How to use `MappedPages`
A key aspect of `MappedPages` is its "access methods" that allow callers to safely reinterpret the underlying mapped memory region as a particular type. 
Reinterpreting untyped memory is a crucial feature for any memory management subsystem; Theseus provides fully-safe interfaces to do so, while existing OSes do not. 
Reinterpretive casting is sometimes also referred to as "struct overlay", as you're overlaying a struct on top of an existing memory region.

| Access method name            | Return type           | Description                                               |
|-------------------------------|-----------------------|------------------------------------|
| [`as_type()`]                 | `&T`                  | returns an immutable reference to a generic type `T` starting at a particular offset into the memory region. |
| [`as_type_mut()`]             | `&mut T`              | same as `as_type()`, but returns a *mutable* reference. |
| [`as_slice()`]                | `&[T]`                | returns a reference to a *slice* (dynamic-sized array) of `N` elements of type `T`, starting at a particular offset. |
| [`as_slice_mut()`]            | `&mut [T]`            | same as `as_slice()`, but returns a mutable reference to a slice of type `T`. |
| [`LoadedSection::as_func()`]  | `& <impl Fn(...)>`    | returns a reference to a function that exists within a `LoadedSection`'s memory region, which must be an executable `.text` section. |


These access methods ensure the aforementioned invariants of the `MappedPages` type.
1. The size of the generic type `T`, which must be known at compile time (`T: Sized`), plus the offset must not exceed the bounds of the memory region.
    * The same is true for slices: the number of elements of a sized type `T: Sized` plus the offset must not exceed the region's bounds.
2. If a mutable reference is requested, the underlying memory region must have been mapped as writable.
    * The same is true for functions and executable memory regions.
3. These methods all return *references* to the requested type or slice, in which the lifetime of the returned reference (`&T`) is dependent upon the lifetime of the `MappedPages` object, in order to prevent use-after-free errors.
    * One cannot obtain an owned instance of a type `T` from an underlying `MappedPages` memory region, because that would remove the semantic connection between the type `T` and the existence of the underlying memory mapping.

In comparison, other OSes typically return raw virtual address values from a memory mapping operation, which you must then unsafely cast to a typed pointer of your choice. 
With raw addresses, there is no lifetime guarantee to ensure that the mapping persists for as long as those virtual addresses are used. 
As such, Theseus removes at compile time the potential to easily cause unsafe, undefined behavior by using a raw virtual address after it has been unmapped.

For more details, see the Theseus paper from OSDI 2020, or Kevin Boos's dissertation, both [available here](../misc/papers_presentations.md#selected-papers-and-theses).


The `MappedPages` type also exposes other convenient utility methods:
* [`remap()`]: change the permissions flags for the virtual pages, which are still mapped to the same physical frames.
* [`merge()`]: combine multiple contiguous `MappedPages` objects into one.
* [`unmap_into_parts()`]: unmap the memory region and re-take ownership of its constituent parts (`AllocatedPages`, etc) for future use.
    * Without calling this, a `MappedPages` object will be auto-unmapped upon drop and its constituent parts deallocated for future use,
      but that will happen behind the scenes without you being able to directly access them.
* You can also call any of the methods from [`PageRange`] as well.


## Deallocating frames
Deallocating virtual pages is easy because the range of `AllocatedPages` is directly stored in and owned by a `MappedPages` object, so it is simply a matter of deallocating them once they are dropped.

However, deallocating a range of `AllocatedFrames` is much more difficult because each page in a range of virtually-contiguous pages may likely be mapped to a different, non-contiguous set of frames.
This means we may have to deallocate many sets of `AllocatedFrames`, up to one per page.

In existing OSes, there is no way to easily and immediately determine which frames are mapped to which virtual pages; this requires a *reverse mapping* from `1` frame to `N` pages, which is prohibitively expensive to maintain.
As such, OS kernels typically run a periodic "garbage collection" thread on idle CPUs that sweeps the page tables to determine which frames can be reclaimed.

However, Theseus's design vastly simplifies the procedure of reclaiming unused physical frames for deallocation.
The single address space design and guaranteed bijective (1-to-1) mappings mean that a frame is mapped *exclusively* by a single page; when that page is no longer mapped to that frame, the frame can be deallocated.
We refer to this as exclusive mappings, and they are realized via a combination of several crates:
1. When frame(s) are unmapped in the [`page_table_entry`] crate, it creates an [`UnmapResult`] that may contain a set of [`UnmappedFrames`].
    * The primary function of interest is [`PageTableEntry::set_unmapped()`].
2. Using strong type safety, the [`frame_allocator`] is able to accept a set of `UnmappedFrames` as a trusted "token" stating that the included frames cannot possibly still be mapped by any pages. It can therefore safely deallocate them.
    * Deallocation occurs seamlessly because an `UnmappedFrames` object can be converted into an `AllocatedFrames` object, [see here for details and source].


<!-- Links below -->
[`PageRange`]: https://theseus-os.github.io/Theseus/doc/memory_structs/struct.PageRange.html
[`frame_allocator`]: https://theseus-os.github.io/Theseus/doc/frame_allocator/index.html

[`AllocatedFrames`]: https://theseus-os.github.io/Theseus/doc/frame_allocator/struct.AllocatedFrames.html
[`AllocatedPages`]: https://theseus-os.github.io/Theseus/doc/page_allocator/struct.AllocatedPages.html
[`MappedPages`]: https://theseus-os.github.io/Theseus/doc/memory/struct.MappedPages.html

[`Mapper`]: https://theseus-os.github.io/Theseus/doc/memory/struct.Mapper.html
[`PageTable`]: https://theseus-os.github.io/Theseus/doc/memory/struct.PageTable.html

[`as_type()`]: https://theseus-os.github.io/Theseus/doc/memory/struct.MappedPages.html#method.as_type
[`as_type_mut()`]: https://theseus-os.github.io/Theseus/doc/memory/struct.MappedPages.html#method.as_type_mut
[`as_slice()`]: https://theseus-os.github.io/Theseus/doc/memory/struct.MappedPages.html#method.as_slice
[`as_slice_mut()`]: https://theseus-os.github.io/Theseus/doc/memory/struct.MappedPages.html#method.as_slice_mut
[`LoadedSection::as_func()`]: https://theseus-os.github.io/Theseus/doc/mod_mgmt/struct.LoadedSection.html#method.as_func
[`remap()`]: https://theseus-os.github.io/Theseus/doc/memory/struct.MappedPages.html#method.remap
[`merge()`]: https://theseus-os.github.io/Theseus/doc/memory/struct.MappedPages.html#method.merge
[`unmap_into_parts()`]: https://theseus-os.github.io/Theseus/doc/memory/struct.MappedPages.html#method.unmap_into_parts

[`page_table_entry`]: https://theseus-os.github.io/Theseus/doc/page_table_entry/index.html
[`UnmapResult`]: https://theseus-os.github.io/Theseus/doc/page_table_entry/enum.UnmapResult.html
[`UnmappedFrames`]: https://theseus-os.github.io/Theseus/doc/page_table_entry/struct.UnmappedFrames.html
[`PageTableEntry::set_unmapped()`]: https://theseus-os.github.io/Theseus/doc/page_table_entry/struct.PageTableEntry.html#method.set_unmapped
[see here for details and source]: https://www.theseus-os.com/Theseus/doc/frame_allocator/fn.init.html#return
[stacks]: https://theseus-os.github.io/Theseus/doc/stack/struct.Stack.html
