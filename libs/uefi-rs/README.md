# uefi-rs

[![Build Status](https://travis-ci.org/rust-osdev/uefi-rs.svg?branch=master)](https://travis-ci.org/rust-osdev/uefi-rs)

## Description

[UEFI] is the successor to the BIOS. It provides an early boot environment for
OS loaders, hypervisors and other low-level applications. While it started out
as x86-specific, it has been adopted on other platforms, such as ARM.

This crates makes it easy to write UEFI applications in Rust.

The objective is to provide **safe** and **performant** wrappers for UEFI interfaces,
and allow developers to write idiomatic Rust code.

Check out @gilomendes [blog post on getting started with UEFI in Rust][gm-blog].

**Note**: due to some issues with the Rust compiler, this crate currently works
and has been tested _only_ with **64-bit** UEFI.

[UEFI]: https://en.wikipedia.org/wiki/Unified_Extensible_Firmware_Interface
[gm-blog]: https://medium.com/@gil0mendes/an-efi-app-a-bit-rusty-82c36b745f49

![uefi-rs running in QEMU](https://imgur.com/SFPSVuO.png)

## Project structure

This project contains multiple sub-crates:

- `uefi` (top directory): defines the standard UEFI tables / interfaces.
  The objective is to stay unopionated and safely wrap most interfaces.

- `uefi-exts`: extension traits providing utility functions for common patterns.
  - Requires the `alloc` crate (either use `uefi-alloc` or your own custom allocator).

- `uefi-macros`: procedural macros that are used to derive some traits in `uefi`.

- `uefi-services`: provides a panic handler, and initializes some helper crates:
  - `uefi-logger`: logging implementation for the standard [log] crate.
    - Prints output to console.
    - No buffering is done: this is not a high-performance logger.
  - `uefi-alloc`: implements a global allocator using UEFI functions.
    - This allows you to allocate objects on the heap.
    - There's no guarantee of the efficiency of UEFI's allocator.

- `uefi-test-runner`: a UEFI application that runs unit / integration tests.

[log]: https://github.com/rust-lang-nursery/log

## Building kernels which use UEFI

This crate makes it easy to start buildimg simple applications with UEFI.
However, there are some limitations you should be aware of:

- The global logger / allocator **can only be set once** per binary.
  It is useful when just starting out, but if you're building a real OS you will
  want to write your own specific kernel logger and memory allocator.

- To support advanced features such as [higher half kernel] and [linker scripts]
  you will want to build your kernel as an ELF binary.

In other words, the best way to use this crate is to create a small binary which
wraps your actual kernel, and then use UEFI's convenient functions for loading
it from disk and booting it.

This is similar to what the Linux kernel's [EFI stub] does: the compressed kernel
is an ELF binary which has little knowledge of how it's booted, and the boot loader
uses UEFI to set up an environment for it.

[higher half kernel]: https://wiki.osdev.org/Higher_Half_Kernel
[linker scripts]: https://sourceware.org/binutils/docs/ld/Scripts.html
[EFI stub]: https://www.kernel.org/doc/Documentation/efi-stub.txt

## Documentation

This crate's documentation is fairly minimal, and you are encouraged to refer to
the [UEFI specification][spec] for detailed information.

[spec]: http://www.uefi.org/specifications

### rustdoc

Use the `build.py` script in the `uefi-test-runner` directory to generate the documentation:

```sh
./build.py doc
```

## Sample code

An example UEFI app is built in the `uefi-test-runner` directory.

Check out the testing [README.md](uefi-test-runner/README.md) for instructions on how to run the crate's tests.

This repo also contains a `x86_64-uefi.json` file, which is a custom Rust target for 64-bit UEFI applications.

## Building UEFI programs

For instructions on how to create your own UEFI apps, see the [BUILDING.md](BUILDING.md) file.

## License

The code in this repository is licensed under the Mozilla Public License 2.
This license allows you to use the crate in proprietary programs, but any modifications to the files must be open-sourced.

The full text of the license is available in the `LICENSE` file.
