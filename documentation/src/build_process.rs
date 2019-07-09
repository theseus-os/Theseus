//! Documentation about how Theseus is built.
//! 
//! # Theseus's Build Process
//! Theseus uses the [cargo virtual workspace](https://doc.rust-lang.org/cargo/reference/manifest.html#the-workspace-section) feature to group all of the crates together into a single meta project, which significantly speeds up build times.     
//! 
//! The top-level Makefile basically just calls the kernel Makefile, copies the kernel build files into a top-level build directory, and then calls `grub-mkrescue` to generate a bootable .iso image.     
//!
//! The kernel Makefile (`kernel/Makefile`) actually builds all of the Rust code using [`xargo`](https://github.com/japaric/xargo), a cross-compiler toolchain that wraps the default Rust `cargo`.
//! The only special action it takes is to build the `nano_core` separately and fully link it against the architecture-specific assembly code in `nano_core/boot` into a static binary.    
//! 
//! ### Debug vs. Release Mode
//! Theseus can be built in a variety of modes, but offers two presets: **debug** and **release** build modes.
//! By default, Theseus is built in debug mode (cargo's "dev" profile) for easy development. 
//! To build in release mode, set the `BUILD_MODE` environment variable when running `make`, like so:    
//! `make run BUILD_MODE=release`    
//! 
//! There is a special file `kernel/Config.mk` that contains the build mode options as well as other configuration options used in the kernel Makefile. 
//! As with most languages, release mode in Rust is *way* faster, but it does take longer to compile and can be difficult to attach a debugger.
//! 
//! 
//! ### `Loadable` Mode: Runtime Loading and Linking of Crates
//! By default, Theseus is built into a single kernel binary just like a regular OS, in which all crates are linked into a single static library and then zipped up into a bootable .iso file. 
//! However, our actual research into runtime composability dictates that all crates (except the `nano_core`) are loaded at runtime, and not linked into a single static kernel binary. 
//!
//! To enable this, use the `make loadable` command to enable the `loadable` feature, which does the following:
//!
//! * Builds each crate into its own separate object file, which are not all linked together like in other OSes.
//! * Enables release mode in order to make each module file smaller and faster to load, i.e., sets `BUILD_MODE=release`.
//! * Copies each crate's object file into the top-level build directory's module subdirectory (`build/grub-isofiles/modules`) such that each module is a separate object file in the final .iso image. 
//!   That allows the running instance of Theseus to see all the modules currently available just by asking the bootloader (without needing a filesystem), and to load them individually.
//! * Sets the `loadable` config option, which as seen in the `nano_core`, will enable the `#![cfg(loadable)]` code blocks that dynamically load other crates rather than include them as static dependencies.
//! 
