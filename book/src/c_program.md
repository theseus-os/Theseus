# Building and Running C programs on Theseus

As Theseus is a safe-language OS that runs all code in a single address space (SAS) and single privilege level (SPL),
there is no guarantee of safety, protection, or isolation when running any other unsafe or non-Rust code directly atop Theseus. 

Nevertheless, we have introduced experimental support for building C programs atop Theseus; proceed at your own risk. 

## Preqrequisites
You must have a version of GCC and Binutils cross-compiled for Theseus, which target the x86_64-elf target and have red-zone usage disabled. 

To make things easy, we have written [an automated script and a guide](building_c_cross_compiler.md) on how to build and install all necessary tools.

Note that the x86_64-elf-* binaries must be on your system PATH before running any of the following gcc commands. 

## Building a C program
See the `c_test` directory for an example dummy C program. All it does is run a simple `main()` function that returns a constant value. 

In short, building a C program requires the following steps:
 1. `make`          -- build Theseus OS itself
 2. `make tlibc`    -- build tlibc: Theseus's libc
 3. `make c_test`   -- build a sample C program
 4. `make orun`     -- run Theseus in QEMU (without rebuilding anything)

## Running a C program
Once the C program's executable ELF file has been packaged into Theseus's ISO image, you can run it in Theseus using the `loadc` application. For example, in Theseus's shell, run the following:
```
loadc /namespaces/_executable/dummy_works
```
You should observe the return value displayed on the shell GUI, as well as various log messages that show output from tlibc alongside those from Theseus's kernel. 


# How does it all work?

