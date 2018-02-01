# Theseus OS

Theseus is a new runtime-composable OS that tackles the problem of *state spill*, the harmful yet ubiquitous phenomenon described in our [research paper from EuroSys 2017 here](http://kevinaboos.web.rice.edu/statespy.html).

We have designed and built Theseus from scratch using Rust to completely rethink state management in an OS, with the intention of avoiding state spill or mitigating its effects to the fullest extent possible. 

The design of Theseus's components and subsystems is frequently inspired by RESTful architectures used across the Web, so there are also references to its previous name `restful_OS` throughout the repository. 


## Building Theseus

Currently, we support building and running Theseus on the following host OSes:
 * Linux distributions, 64-bit, preferably Debian-based like Ubuntu.
 * Windows using the Windows Subsystem for Linux (WSL), Ubuntu version.
 * Mac OS X is not currently supported officially, but it should be possible. 


### Setting up Rust

Install the current Rust compiler and toolchain by following the [setup instructions here](https://www.rust-lang.org/en-US/install.html).
Or, basically just run this command and follow the instructions:   
`$ curl https://sh.rustup.rs -sSf | sh`

Because OS development requires many language features that Rust considers to be unstable, you must use a nightly compiler. You can accomplish this with:   
`$ rustup default nightly`

Since we're cross compiling for a custom target triple (the Theseus platform), we need to install the Rust source code:   
`$ rustup component add rust-src`

We also need to install Xargo, a drop-in replacement wrapper for Cargo that makes cross-compiling easier:    
`$ cargo install xargo`


### Finishing the build
You will need to install the following packages:  
`$ sudo apt-get install nasm grub-pc-bin mtools xorriso qemu`   

If you're on Windows using WSL, you'll need to do the following:
  * Install an X Server for Windows; we suggest using [Xming](https://sourceforge.net/projects/xming/).
  * Set an X display, by running `export DISPLAY=:0`. You'll need to do this each time you open up a new WSL terminal, so it's best to add it to your .bashrc file. You can do that with `echo "export DISPLAY=:0" >> ~/.bashrc`.

When you first check out the project, don't forget to checkout all the submodule repositories too:
`$ git submodule update --init --recursive`

To build and run Theseus in QEMU, simply run:   
`$ make run`

To run it again without rebuilding the whole project:   
`$ make orun`

Run `make help` to see other make tarets. 


### Note: Rust compiler versions
Because we use the Rust nightly compiler (not stable), the Theseus Makefile checks to make sure that you're using the same version of Rust that we are. We were inspired to add this safety check when we failed to build other Rust projects put out there on Github because they used an earlier version of the nightly Rust compiler than what we had installed on our systems. To avoid this undiagnosable problem, we force you to use the latest version of `rustc` that is known to properly build Theseus. This version is upgraded as often as possible to align with the latest Rust nightly, but this is a best-effort policy.

So, if you see a build error about the improper version of `rustc`, follow the instructions printed out at the end of the error message.     


## Using QEMU 
QEMU allows us to run Theseus quickly and easily in its own virtual machine, completely segregated from the host machine and OS. 
To exit Theseus in QEMU, press `Ctrl+Alt`, which releases your keyboard focus from the QEMU window. Then press `Ctrl+C` in the terminal window that you ran `make run` from originally to kill QEMU. 

To investigate the state of the running QEMU entity, you can switch to the QEMU console by pressing `Ctrl+Alt+2`. Switch back to the main window with `Ctrl+Alt+1`.    

#### KVM Support
While not strictly required, KVM will speed up the execution of QEMU.
To install KVM, run the following command:  `sudo apt-get install kvm`.  
If you have KVM installed, it will automatically be enabled by our Makefile.



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
Our personal preference is to use Visual Studio Code (VS Code), which has excellent, official support from the Rust language team. Other options are available [here](https://areweideyet.com/), but we don't recommend them.

For VS Code, recommended plugins are:
 * Rust (rls), by rust-lang
 * Better TOML, by bungcip
 * x86 and x86_64 Assembly, by 13xforever


## License
The source code is licensed under the MIT License. See the LICENSE-MIT file for more. 
