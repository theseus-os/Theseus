# Theseus Tools

This directory contains tools used in Theseus's build process or for testing purposes. 

## Build-related tools
* `copy_latest_crate_objects`: a Rust program that selects the latest version of a compiled crate object file and copies it to the OS image for creating a GRUB image. 
* `demangle_readelf_file`: a Rust program that demangles the output of `readelf`.
* `grub_cfg_generation`: a Rust program that autogenerates a multiboot2-compliant grub.cfg file for GRUB, specifying which multiboot2 modules should be included in the ISO.
* `theseus_xargo`: a wrapper around cargo that cross-compiles Theseus components, handles out-of-tree builds based on a set of pre-built Theseus .rlibs,  and performs special "partially-static" linking procedures.

## Other tools
* `diff_crates`: a Rust program that identifies the differences in crate object files across two different Theseus builds, for purposes of creating a live evolution manifest.
* `receive_udp_messages`: a test tool for receiving messages over UDP. Not really used any more. 
* `sample_parser`: a tool for parsing the output of an execution trace of PMU samples.

