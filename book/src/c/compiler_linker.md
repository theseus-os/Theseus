# Compiling and Linking C programs

> *Warning:* Support for building C programs atop Theseus is experimental and liable to change at any moment.

Currently, we use a custom form of "split" linking that is both fully-static at compile time *and* partially-dynamic at runtime.

## 1. Full static linking at build time
This procedure can be broken down into the following steps:

1. Compile `tlibc` as a standard Rust crate into a series of object files, 
2. Combine or link those tlibc-related as a single archive or object file,
3. Compile the basic C program, e.g., `dummy.c`,
4. Statically link the C program to the prebuilt `tlibc` object to produce a standalone executable.

The first two steps were [described previously](tlibc.md); the latter two are described below.

To build and link a dummy C program to tlibc, the invocation of `gcc` currently requires several arguments to customize the compiler's behavior as well as its usage of the `ld` linker. The key arguments are shown and described below:
```sh
x86_64-elf-gcc                             \
    -mno-red-zone -nostdlib -nostartfiles  \
    -ffunction-sections -fdata-sections    \
    -mcmodel=large                         \
    -Wl,-gc-sections                       \
    -Wl,--emit-relocs                      \
    -o dummy_works                         \
    path/to/crtbegin.o                     \
    dummy.c                                \
    path/to/tlibc.o                        \
    path/to/crtend.o
```

The most important arguments are:
* `-mno-red-zone`, `-mcmodel=large`:  match Theseus's Rust-side compiler configuration so the C code can properly invoke the Rust code.
* `-Wl,--emit-relocs`: include details in the ELF executable (`.rela` sections) about the specific relocation actions the linker performed.

After the above gcc command, we have a standalone executable ELF file `dummy_works`.


## 2. Partial dynamic re-linking at runtime
The ELF executable `dummy_works` can immediately be loaded and executed atop Theseus, but it won't necessarily work properly and execute as expected. 
This is because it was fully statically linked, meaning that the executable includes duplicate instances of the same data and function sections that already exist in the loaded instances of Theseus crates in memory (cells).

Most importantly, those data sections represent system-wide singleton states (`static` variables in Rust) that have *already been initialized* and are in active use by all other Theseus components. 
Thus, the data instances packaged into the executable have *not* been initialized and can't safely be used. 
Using those sections would result in multiple copies of data that's supposed to be a system-wide singleton; this would be bad news for most Theseus components, e.g., frame allocator's system-wide list of free physical memory. 

To solve this problem, we re-perform (overwrite) all of the relocations in the executable ELF file such that they refer to the *existing sections* already loaded into Theseus instead of the new uninitialized/unused ones in the executable itself. 
This only applies for sections that already exist in Theseus; references to new sections that are unique to the executable are kept intact, of course.
The relocation information is encoded into the ELF file itself as standard `.rela.*` sections via the `--emit-relocs` linker argument shown above.

This procedure is currently performed by the `loadc` application; it also handles loading the ELF executable segments (program headers) and managing their metadata. 
