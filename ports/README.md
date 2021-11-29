# `ports/`: Third-party Libraries Ported to Theseus

This directory contains libraries that have been ported to use Theseus-specific functions and types, and can depend directly upon Theseus crates in `kernel/`.
This is in contrast to `libs/`, which contains third-party libraries that are standalone and cannot depend on Theseus crates.

Most of these folders are git submodules (separate repositories) in order to preserve the history of each crate and link back to the original crate.
