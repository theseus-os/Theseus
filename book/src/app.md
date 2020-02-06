# Application Support and Development

One of the unusual features of Theseus, compared to mainstream operating systems like Linux, is that safe applications can be loaded into the same address space as the kernel and run at the kernel privilege level. Below we provide information about how such apps are supported by Theseus and are developed.

## Dynamic Linking and Loading of app crates

Currently, all applications run diretly in kernel space at the same privilege level (Ring 0).
Applications are simple object files that are loaded into the kernel address space, just like any other kernel crate.
The only real distinction is that they must use only safe code (unsafe code is forbidden),
and they must expose an entry point function named `main`, shown below.

```rust
#[no_mangle]
pub fn main(args: Vec<String>) -> isize { ... }
```
