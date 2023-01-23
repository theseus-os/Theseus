# Theseus Tools

This directory contains tools used in Theseus's build process or for testing purposes. 

## Build-related tools
* `copy_latest_crate_objects`: a Rust program that selects the latest version of a compiled crate object file and copies it to the OS image for creating a GRUB image. 
* `demangle_readelf_file`: a Rust program that demangles the output of `readelf`.
* `limine_compress_modules`: a Rust program that takes all object files generated from a Theseus build and compresses them into a single archive. 
    * This is needed when using the `limine` bootloader, which doesn't readily support booting an OS with hundreds of boot modules.
    * This may also offer performance improvements for GRUB when booting Theseus, but it is not enabled by default.
* `serialize_nano_core`: A Rust program that creates a serialized representation of the symbols in the `nano_core` binary from the output of `demangle_readelf_file`. 
* `grub_cfg_generation`: a Rust program that autogenerates a multiboot2-compliant grub.cfg file for GRUB, specifying which multiboot2 modules should be included in the ISO.
* `theseus_cargo`: a wrapper around cargo that supports out-of-tree builds for arbitrary crates that are cross-compiled against an existing build of Theseus. In the future, it will also perform special "partially-static" linking procedures.
* `uefi_builder`: A (collection of) Rust program(s) that generates the necessary files to boot Theseus using UEFI. See `uefi_builder/README.md` for more details on why each target requires its own program.

## Other tools
* `diff_crates`: a Rust program that identifies the differences in crate object files across two different Theseus builds, for purposes of creating a live evolution manifest.
* `receive_udp_messages`: a test tool for receiving messages over UDP. Not really used any more. 
* `sample_parser`: a tool for parsing the output of an execution trace of PMU samples.

