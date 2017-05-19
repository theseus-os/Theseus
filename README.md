# Theseus OS

Theseus is a new OS that tackles the problem of *state spill*, the harmful yet ubiquitous phenomenon described in our [research paper from EuroSys 2017 here](http://kevinaboos.web.rice.edu/statespy.html).

We have build Theseus from scratch using Rust to completely rethink state management in an OS, with the intention of avoiding state spill or mitigating its effects to the fullest extent possible. 

The design of Theseus's components and subsystems is frequently inspired by RESTful architectures used across the Web, so there are also references to its previous name `restful_OS` throughout the repository. 


## Setting up Rust

As our OS is based off of [Philipp Oppermann's fantastic Blog OS](htpp://os,phil-opp.com), our setup process is similar to his (taken in part from [his instructions](http://os.phil-opp.com/set-up-rust.html)). We highly encourage you to follow along with his excellent series of blog posts to understand the initial boot-up procedure of our OS on x86-64 architectures. 

You will need the current Rust compiler and toolchain by following the [setup instructions here](https://www.rust-lang.org/en-US/install.html).

Basically, just run this command and follow the instructions:   
`$ curl https://sh.rustup.rs -sSf | sh`

Because OS development requires many language features that Rust considers to be unstable, you must use a nightly compiler. You can accomplish this with:   
`$ rustup default nightly`

Since we're cross compiling for a custom target triple, we need to install the Rust source code:   
`$ rustup component add rust-src`

We also need to install Xargo, a drop-in replacement wrapper for Cargo that makes cross-compiling easier:   
`$ cargo install xargo`


## Additional Build Environment Setup
Currently we only support building on 64-bit Debian-like Linux distributions (e.g., Ubuntu 16.04). You will need to install the following packages:  
`$ sudo apt-get install nasm grub-mkrescue grub-pc-bin mtools xorriso qemu`   

To build and run Theseus in QEMU, simply run:   
`$ make run`

To run it without rebuilding the whole project:   
`$ make orun`


## Debugging 
GDB has built-in support for QEMU, but it doesn't play nicely with OSes that run in long mode. In order to get it working properly with our OS in Rust, we need to patch it and build it locally. The hard part has already been done for us ([details here](http://os.phil-opp.com/set-up-gdb.html)), so we can just quickly set it up with the following commands.  

First, install the following packages:  
`$ sudo apt-get install texinfo flex bison python-dev ncurses-dev`

Then, from the base directory of the Theseus project, run this command to easily download and build it from an existing patched repo:  
`$ curl -sf https://raw.githubusercontent.com/phil-opp/binutils-gdb/rust-os/build-rust-os-gdb.sh | sh`  

After that, you should have a `rust-os-gdb` directory that contains the `gdb` executables and scripts. 

Then, simply run `make debug` to build and run Theseus in QEMU, which will pause the OS's execution until we attach our patched GDB instance.   
To attach the debugger to our paused QEMU instance, run `make gdb` in another terminal. QEMU will be paused until we move the debugger forward, with `n` to step through or `c` to continue running the debugger.  
Try setting a breakpoint at the kernel's entry function using `b rust_main` or at a specific file/line, e.g., `b scheduler.rs:40`.

## IDE Setup
The developer's personal preference is to use Visual Studio Code (VS Code), which is officially supported on Rust's [Are We IDE Yet](https://areweideyet.com/) website. Other good options include Atom, Sublime, Eclipse, and Intellij, but we have had the most success developing with VS Code. You'll need to install several plugins, like racer and rust-fmt, to allow whichever IDE you choose to properly understand Rust source code.


## License
The source code is licensed under the MIT License. See the LICENSE-MIT file for more. 
