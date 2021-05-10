# Configuring Theseus at Build Time

There is a top-level `build.rs` file that 

If you want your crate to depend on any cfg values, you must include this `build.rs` file in your crate's `Cargo.toml` file. 
This typicall looks like so:
```toml
[package]
authors = ["Kevin Boos <kevinaboos@gmail.com>"]
name = "my_crate"
description = "brief description of my_crate here"
version = "0.1.0"
build = "../../build.rs"
```
The last line in the above TOML snippet is what informs cargo that it should run that build script before compiling `your_crate`, which ensures the cfg values specified in `THESEUS_CONFIG` will actually be activated.

TODO: explain the `THESEUS_CONFIG` environment variable, and how you can use it in the source code with `cfg()` attributes.
