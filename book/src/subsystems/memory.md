# Memory Management in Theseus

TODO: Single address space, etc.

## Physical memory management
TODO: frame allocator description

## Virtual memory management
TODO: page allocator description

## Mapping virtual memory to physical memory
Page table abstraction plus mapper abstraction. 

### The `MappedPages` type

TODO: MappedPages is the fundamental type to use for accessing and managing regions of memory. It is the backing type behind/beneath ALL other memory objects in Theseus, including stacks, heaps, device memory, MMIO regions, etc.

TODO: describe invariants and intralingual implementation. 

