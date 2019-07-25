# Application Support and Development

One of the unusual features of Theseus, compared to mainstream operating systems like Linux, is that safe applications can be loaded into the same address space as the kernel and run at the kernel privilege level. Below we provide information about how such apps are supported by Theseus and are developed.

## Dynamic Linking and Loading of app crates

Currently, all applications run diretly in kernel space at the same privilege level (Ring 0).
Applications are simple object files that are loaded into the kernel address space, just like any other kernel crate.
Unlike kernel crates, their publicly-exposed symbols are *not* added to the namespace's symbol map, so they cannot be directly invoked by other app or kernel crates.
(Note that "singleton" applications are supported as a hack to get around this limitation for the time being, until more robust standard library and libc support is added.)
The only other distinctions are that they must use only safe code (unsafe code is forbidden),
and they must expose an unmangled entry point, defined as below.

```rust
#[no_mangle]
pub fn main(args: Vec<String>) -> isize { ... }
```
