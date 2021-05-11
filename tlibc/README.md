# `tlibc`: Theseus's libc
This directory contains a custom libc implementation that targets the Theseus OS platform. Currently, `tlibc` is a work-in-progress and a proof of concept; it is missing most standard libc functionality. 

Large portions of `tlibc` code have been borrowed from Redox's [relibc](https://gitlab.redox-os.org/redox-os/relibc) to get up and running quickly, but will be replaced over time with equivalent Theseus-specific code.

It requires a cross-compiled version of `gcc` and `binutils` that will compile C code targeted at Theseus. 
[Read more about how to build those here](../BuildingCrossCompiler.md).

### Manual build fixes
Currently, to get everything to build from scratch properly,  we need to add the following manual steps to ensure the build process works:
 1. Copy the `[patch.crates-io]` section from the top-level Theseus `Cargo.toml` file to the local `tlibc/Cargo.toml` file. 
 2. Attempt to build the tlibc project by running `cargo clean && ./build.sh`.
 3. Adjust the contents of the `Cargo.lock` file to ensure that all of the crate dependencies have the same version as what's being used by the main Theseus build. For example:
    * qp-trie  (changed to use the theseus-os fork on github)
	* linked_list_allocator   (changed to use version 0.8.6 instead of 0.8.11)


## Building as part of Theseus
Note that the above [manual build fixes](#Manual-build-fixes) have already been done if you're checking this out from GitHub, so the Cargo.toml and Cargo.lock files should include the above necessary changes. 

You should merely have to run `make tlibc` from the top-level Theseus directory. 

To test it out with an actual C executable, run the following series of instructions:
```sh
# build the main Theseus ISO
make
# build tlibc and package it into the ISO
make c_test
# build a dummy C executable, statically link it to tlibc, and package it into the ISO
make tlibc
# run the ISO in QEMU, without rebuilding
make orun
```
