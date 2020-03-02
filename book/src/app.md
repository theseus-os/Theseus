# Application Support and Development

One of the unusual features of Theseus, compared to mainstream operating systems like Linux, is that safe applications can be loaded into the same address space as the kernel and run at the kernel privilege level. Below we provide information about how such apps are supported by Theseus and are developed.

## Dynamic Linking and Loading of Application Crates

Currently, all applications run directly in kernel space at the same privilege level (Ring 0).
Applications are simply object files that are loaded into the kernel address space, just like any other kernel crate.
The only real distinction is that they must use only safe code (unsafe code is forbidden),
and they must expose a **public** entry point function named `main`, shown below.
If the `main` function is not `pub`, it will be removed by compiler optimizations and the resulting application crate will not be runnable.

```rust
pub fn main(args: Vec<String>) -> isize { ... }
```

Note that application-level libraries do not need to expose a `main` function;
only applications that intend to be run as binary executables do. 