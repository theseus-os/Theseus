# `tlibc`: Theseus's libc
This directory contains a custom libc implementation that targets the Theseus OS platform. 

It requires a cross-compiled version of `gcc` and `binutils` that will compile C code targeted at Theseus. 
[Read more about how to build those here](../BuildingCrossCompiler.md).

## Manual build fixes
Currently, to get everything to build from scratch properly,  we need to add the following manual steps to ensure the build process works:
 1. Copy the `[patch.crates-io]` section from the top-level Theseus `Cargo.toml` file to the local `tlibc/Cargo.toml` file. 
 2. Attempt to build the tlibc project by running `cargo clean && ./build.sh`.
 3. Adjust the contents of the `Cargo.lock` file to ensure that all of
    * qp-trie  (changed to use the theseus-os fork on github)
	* linked_list_allocator   (changed to use version 0.8.6 instead of 0.8.11)

Note that all of this has already been done if you're checking this out from GitHub, so the Cargo.toml and Cargo.lock files should include the above changes already. 
