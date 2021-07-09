# Building and Running C programs on Theseus

> *Warning:* Support for building C programs atop Theseus is experimental and liable to change at any moment.

As Theseus is a safe-language OS that runs all code in a single address space (SAS) and single privilege level (SPL),
**there is no guarantee** of safety, protection, or isolation when running any other unsafe or non-Rust code directly atop Theseus. 

Nevertheless, we have introduced experimental support for building C programs atop Theseus; proceed at your own risk. 

## Prerequisites
You must have a version of GCC and Binutils cross-compiled for Theseus, e.g., the `x86_64-elf` target with `red-zone` usage disabled. 

To make things easy, we have written [an automated script and a guide](cross_compiler.md) on how to build and install all necessary tools.

Note that the `x86_64-elf-*` binaries must be on your system PATH before running any of the following gcc commands. 

## Quickstart: Building a C program
See the `c_test` directory for an example dummy C program. All it does is run a simple `main()` function that returns a constant value. 

In short, building a C program requires the following steps:
```sh 
make         # 1. Build Theseus OS itself
make tlibc   # 2. Build tlibc, Theseus's libc
make c_test  # 3. Build a sample C program
make orun    # 4. Run Theseus in QEMU (without rebuilding anything)
```

## Running a C program
Once the C program's executable ELF file has been packaged into Theseus's ISO image, you can execute it in Theseus using the `loadc` application. Executables are automatically placed in the `_executable` namespace folder by default, so run the following in Theseus's shell:
```
loadc /namespaces/_executable/dummy_works
```
You should observe the return value displayed on the shell GUI, as well as various log messages that show output from tlibc alongside those from Theseus's kernel. 


# How does it all work?

The following sections describe how to set up the toolchain, how `tlibc` is built, and how C programs are compiled and linked.
