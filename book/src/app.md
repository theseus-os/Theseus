# Application Support and Development
One of the unusual features of Theseus, compared to mainstream operating systems like Linux, is that safe applications are loaded into the same single address space as the rest of the OS and run at the same kernel privilege level. Below, we provide information about how such apps are supported by Theseus and how you can develop a new app.


## Dynamic Linking and Loading of Application Crates
Applications are simply object files that are loaded into the  address space, just like any other kernel crate.
The only real distinction is that they must use only safe code (unsafe code is forbidden),
and they must expose a **public** entry point function named `main`, shown below.
If the `main` function is not `pub`, it may be removed by compiler optimizations or undiscoverable by the application loader code, 
causing the application crate to be non-runnable.

```rust,no_run,no_playground
pub fn main(args: Vec<String>) -> isize { ... }
```

Note that application-level *libraries* do not need to expose a `main` function;
only applications that intend to be run as binary executables do. 

If you forget to include a `main()` function in your application, the crate manager in Theseus will load and link it successfully but fail to run it; a runtime error will be thrown. 


## Creating and building a new application
Theseus's build system will automatically build any crates in the `applications/` directory, so all you have to do is place your new application crate there. 
The name of the directory holding your crate files **must be the same** as the name of the crate as specified in its Cargo.toml `name` field. 

So, for example, you could create a new application crate called `my_app` with the following file structure:
```
applications/
├── my_app
│   ├── Cargo.toml
│   └── src
│       └── lib.rs
├── ...
```

The `applications/my_app/src/lib.rs` file contains the application code with at least a `fn main()` body (as shown above). 
The `applications/my_app/Cargo.toml` file **must specify the same name as the containing directory**:
```toml
[package]
name = "my_app"
...
```

After building and running Theseus, you can type `my_app` into the Theseus shell to run the application as expected.


## Examples
See the many examples in the `applications/` directory. The `example` application is designed to serve as a starting point for your new application that you can easily duplicate. We offer a ported version of `getopts` to help parse command-line arguments. 


## Dependencies: how to use OS functionality
Currently, applications can use any Theseus kernel crate as a direct dependency (via its `Cargo.toml` file). This is a temporary design choice to bridge the lack of a real standard library. 

In the future, this will be replaced with `libtheseus` in combination with Rust's standard library, in which applications can *only* access the kernel functionality re-exported by `libtheseus` and any functionality offered by the Rust standard library, which has two benefits:
* Applications will not be able to access public but "sensitive" kernel functions unless they are explicitly made visible to applications via the `libtheseus` library.
* Applications will not have to know which kernel crate provides a specific feature; they can simply depend on the single `libtheseus` crate to access any OS feature. Their dependency management will be very simple. 