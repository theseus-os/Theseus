# How to contribute to uefi-rs

Pull requests, issues and suggestions are welcome!

The UEFI spec is huge, so there might be some omissions or some missing features.
You should follow the existing project structure when adding new items.

## Workflow

First, change to the `uefi-test-runner` directory:

```shell
cd 'uefi-test-runner'
```

Please take a quick look at the README for an overview of the system requirements
of the test runner.

Make some changes in your favourite editor / IDE:
I use [VS Code][code] with the [RLS][rls] extension.

Test your changes:

```shell
./build.py run
```

The line above will open a QEMU window where the test harness will run some tests.

Any contributions are also expected to pass [Clippy][clippy]'s static analysis,
which you can run as follows:

```shell
./build.py clippy
```

[clippy]: https://github.com/rust-lang-nursery/rust-clippy
[code]: https://code.visualstudio.com/
[rls]: https://github.com/rust-lang-nursery/rls-vscode

## Style guide

This repository follows Rust's [standard style][style], the same one imposed by `rustfmt`.

You can apply the standard style to the whole package by running `cargo fmt --all`.

[style]: https://github.com/rust-lang-nursery/fmt-rfcs/blob/master/guide/guide.md

## UEFI pitfalls

Interfacing with a foreign and unsafe API is a difficult exercise in general, and
UEFI is certainly no exception. This section lists some common pain points that
you should keep in mind while working on UEFI interfaces.

### Enums

Rust and C enums differ in many way. One safety-critical difference is that the
Rust compiler assumes that all variants of Rust enums are known at compile-time.
UEFI, on the other hand, features many C enums which can be freely extended by
implementations or future versions of the spec.

These enums must not be interfaced as Rust enums, as that could lead to undefined
behavior. Instead, integer newtypes with associated constants should be used. The
`newtype_enum` macro is provided by this crate to ease this exercise.

### Pointers

Pointer parameters in UEFI APIs come with many safety conditions. Some of these
are usually expected by unsafe Rust code, while others are more specific to the
low-level environment that UEFI operates in:

- Pointers must reference physical memory (no memory-mapped hardware)
- Pointers must be properly aligned for their target type
- Pointers may only be NULL where UEFI explicitly allows for it
- When an UEFI function fails, nothing can be assumed about the state of data
  behind `*mut` pointers.
