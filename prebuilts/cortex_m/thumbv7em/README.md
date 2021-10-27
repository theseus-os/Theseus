# libcortex_m

## Description
This directory contains a pre-compiled library for use with the [cortex-m](https://crates.io/crates/cortex-m) crate from crates.io. The crate requires an architecture-specific library to be linked in the build process, but due to an error in the `build.rs` script of the crate, the library cannot be linked properly on custom targets. Thus, we must [override the build script](https://doc.rust-lang.org/cargo/reference/build-scripts.html#overriding-build-scripts) and manually link in `libcortex-m.a`.

## Code Source
The `libcortex-m.a` file in this directory is a copy of the library file for `thumbv7em` devices included with `cortex-m` that is normally linked at build time (source [here](https://github.com/rust-embedded/cortex-m/blob/master/bin/thumbv7em-none-eabi.a)).

## Licensing
The `cortex-m` crate is released under the MIT license, which allows it to be copied freely, provided the associated software includes the MIT license. As Theseus is released under the MIT license, we are thereby permitted to make use of the libraries included in this directory. For more information, see the [MIT license](https://opensource.org/licenses/MIT).