# Theseus's Build Process

### Cargo
Theseus uses [cargo](https://doc.rust-lang.org/cargo/index.html), Rust's package manager and build tool, to automatically manage dependencies and invoke the actual Rust compiler for us.
We utilize cargo's [workspace feature](https://doc.rust-lang.org/cargo/reference/workspaces.html) with a virtual manifest to group all of the main crates together into a single top-level meta project, which significantly speeds up build times.
As such, the crates from the main repository folders (`kernel/` and `applications/`) and all of their dependencies are all compiled into a single `target/` folder.

The members of this workspace are defined in the root [Cargo.toml](https://github.com/theseus-os/Theseus/blob/theseus_main/Cargo.toml) manifest file, plus the list of other folders that should be ignored by cargo.


### Makefiles
Although we use cargo to build all Rust code, we still use `make` and Makefiles to handle high-level build tasks. You should never need to directly run `cargo` or `rustc` commands; go through `make` instead. 

The top-level [Makefile](https://github.com/theseus-os/Theseus/blob/theseus_main/Makefile) essentially just invokes the Rust toolchain and compiler via `cargo`, then copies the compiled object files from the appropriate `target/` directory into the top-level `build/` directory, and finally generates a bootable `.iso` image using various bootloader tools, e.g., GRUB.

The only special build action the Makefile takes is to use the `nasm` assembler to compile the  architecture-specific assembly code in `nano_core/boot/`, and then fully link that against the `nano_core` into a separate static binary.


### Configuring Theseus
Continue on to [the next section](configuration.md) to read more about configuring Theseus's build.
