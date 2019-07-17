# Creating UEFI applications

UEFI applications are simple COFF (Windows) executables, with the special
`EFI_Application` subsystem, and some limitations (such as no dynamic linking).

The `x86_64-uefi.json` file describes a custom target for building UEFI apps.

## Prerequisites

- [cargo-xbuild](https://github.com/rust-osdev/cargo-xbuild): this is essential
  if you plan to do any sort of cross-platform / bare-bones Rust programming.

## Steps

The following steps allow you to build a simple UEFI app.

- Create a new `#![no_std]` binary, add `#![no_main]` to use a custom entry point,
  and make sure you have an entry point function which matches the one below:

```rust
#[no_mangle]
pub extern "win64" fn uefi_start(handle: Handle, system_table: SystemTable<Boot>) -> Status;
```

- Copy the `uefi-test-runner/x86_64-uefi.json` target file to your project's root.
  You can customize it.

- Build using `cargo xbuild --target x86_64-uefi`.

- The `target` directory will contain a `x86_64-uefi` subdirectory,
  where you will find the `uefi_app.efi` file - a normal UEFI executable.

- To run this on a real computer:
  - Find a USB drive which is FAT12 / FAT16 / FAT32 formatted
  - Copy the file to the USB drive, to `/EFI/Boot/Bootx64.efi`
  - In the UEFI BIOS, choose "Boot from USB" or similar

- To run this in QEMU:
  - You will need a recent version of QEMU as well as OVMF to provide UEFI support
  - Check the `build.py` script for an idea of what arguments to pass to QEMU

You can use the `uefi-test-runner` directory as sample code for building a simple UEFI app.
