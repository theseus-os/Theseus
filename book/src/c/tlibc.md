# `tlibc`: Compiling and Linking Theseus's libc

> *Warning:* Support for building C programs atop Theseus is experimental and liable to change at any moment.

Theseus's libc implementation, `tlibc`, is a work in progress and currently a proof-of-concept library that's missing most standard libc functionality. 

## Building tlibc in a Theseus-compatible way
Most standard library and libc implementations are built as fully-linked static or dynamic libraries; in Rust terms, this corresponds to the `staticlib` or `cdylib` crate type ([see more about crate types and linkage here](https://doc.rust-lang.org/reference/linkage.html)).

This doesn't work well for Theseus for a few reasons. 
First, because Theseus runs everything in a single privilege level, there is no clear point of separation between the lowest level of user code and the highest level of kernel code.
In conventional OSes, standard libraries use the *system call* interface to separate their code from the rest of the OS. 
Therein, building against a specific OS platform is easy -- you simply define the system call interface and compile against any necessary header files.
There is no complex linking that needs to occur, since the lowest level of the dependency chain ends at the `syscall` assembly instruction, which makes the library self-contained from the linker's point of view.


Second, Theseus dynamically links raw object files at runtime, so we cannot easily create a fully statically-linked binary for a standalone C library because it won't know where its dependencies will exist in memory.
Again, this is not a problem for standard libc implementations since it doesn't need to directly link against each specific syscall handler function. 

Thus, we use the standard `rlib` crate type for `tlibc` and perform partial linking of the raw compiled object files ourselves. 
```sh 
ld -r -o tlibc/target/.../tlibc.o  tlibc/target/.../deps/*.o
```
Alternatively, we could also use `ar` to create an archive of all of the object files, as shown below; there's not much of a functional difference between the two approaches, but some build tools prefer `.a` archives instead of a `.o` object files. 
```sh
ar -rcs tlibc/target/.../libtlibc.a  tlibc/target/.../deps/*.o 
```

We use the `theseus_cargo` tool ([as described here](../building/rust_builds_out_of_tree.md)) to ensure that `tlibc` is compiled against and depends on the correct version of crates and symbols from an existing Theseus build. 

## Using tlibc
Once we have the `tlibc.o` (or `.a`) file, we can use that to satisfy any C program's dependencies on basic libc functions/data.

[The next section](compiler_linker.md) describes how we use the `tlibc` file to build a standalone C executable that can run atop Theseus.
