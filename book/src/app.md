# Application Support and Development
One of the unusual features of Theseus, compared to mainstream operating systems like Linux, is that safe applications can be loaded into the same address space as the kernel and run at the kernel privilege level. Below we provide information about how such apps are supported by Theseus and are developed.


## Dynamic Linking and Loading of Application Crates
Currently, all applications run directly in kernel space at the same privilege level (Ring 0).
Applications are simply object files that are loaded into the kernel address space, just like any other kernel crate.
The only real distinction is that they must use only safe code (unsafe code is forbidden),
and they must expose a **public** entry point function named `main`, shown below.
If the `main` function is not `pub`, it may be removed by compiler optimizations or undiscoverable by the application loader code, 
causing the application crate to be non-runnable.

```rust
pub fn main(args: Vec<String>) -> isize { ... }
```

Note that application-level *libraries* do not need to expose a `main` function;
only applications that intend to be run as binary executables do. 


## Example
See the many examples in the `applications/` directory. The `example` application is designed to serve as a starting point for your new application that you can easily duplicate. We offer a ported version of `getopts` to help parse command-line arguments. 


## Dependencies: how to use OS functionality
Currently, applications can use any Theseus kernel crate as a direct dependency (via its `Cargo.toml` file). This is a temporary design choice to bridge the lack of a real standard library. 

In the future, this will be replaced with `libtheseus` in combination with Rust's standard library, in which applications can *only* access the kernel functionality re-exported by `libtheseus` and any functionality offered by the Rust standard library, which has two benefits:
* Applications will not be able to access public but "sensitive" kernel functions unless they are explicitly made visible to applications via the `libtheseus` library.
* Applications will not have to know which kernel crate provides a specific feature; they can simply depend on the single `libtheseus` crate to access any OS feature. Their dependency management will be very simple. 