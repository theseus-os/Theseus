# Theseus OS

Theseus is a new OS written from scratch in [Rust](https://www.rust-lang.org/) to experiment with novel OS structure, better state management, and how to shift OS responsibilities like resource management into the compiler. 

We are continually working to improve the OS, including its fault recovery abilities for higher system availability without redundancy, as well as easier and more arbitrary live evolution and runtime flexbility.
Though still an incomplete prototype, we envision that Theseus will be useful for high-end embedded systems or edge datacenter environments. 
See our [published papers](http://kevinaboos.web.rice.edu/publications.html) for more information about Theseus's design principles and implementation philosophy, as well as our goal to avoid the phenomenon of *state spill* or mitigate its effects as much as possible.
Also, see Theseus's [documentation](#Documentation) for more.




# Building Theseus
When you first check out the project, don't forget to checkout all the submodule repositories too:    
`git submodule update --init --recursive`

Currently, we support building and running Theseus on the following host OSes:
 * Linux, 64-bit Debian-based distributions like Ubuntu, tested on Ubuntu 16.04, 18.04, 20.04.
 * Windows, using the Windows Subsystem for Linux (WSL), tested on the Ubuntu version of WSL and WSL2.
 * MacOS, tested on versions High Sierra (10.13) and Catalina (10.15.2).
 * Docker, atop any host OS that can run a Docker container.


## Setting up the build environment

### Linux or WSL (Windows Subsystem for Linux)
  * Update your system's package list:    
    `sudo apt-get update`.    
  * Install the following packages:    
    `sudo apt-get install nasm pkg-config grub-pc-bin mtools xorriso qemu qemu-kvm`     

If you're on WSL, also do the following:
  * Install an X Server for Windows; we suggest using [Xming](https://sourceforge.net/projects/xming/).
  * Set an X display by running `export DISPLAY=:0`.    
    You'll need to do this each time you open up a new WSL terminal, so it's best to add it to your `.bashrc` file. You can do that with:    
    `echo "export DISPLAY=:0" >> ~/.bashrc`.
  * If you get this error:    
    `Could not initialize SDL(No available video device) - exiting`
    then make sure that your X Server is running before running `make run`, and that you have set the `DISPLAY` environment variable above.
  * Install a C compiler and linker toolchain, such as `gcc`.

### MacOS
  * Install [MacPorts](https://www.macports.org/install.php) and [HomeBrew](https://brew.sh/). 
  * run the MacOS build setup script:    
    `./scripts/mac_os_build_setup.sh`
  * If things later go wrong, run     
    `rm -rf $HOME/theseus_tools_src $HOME/theseus_tools_opt /opt/local/bin/make`     
    and then start over. 

### Docker
Note: building and running Theseus within a Docker container may be slower than on a native host OS (e.g., QEMU+KVM). 
  * Install [Docker Engine](https://docs.docker.com/engine/install/). (Skip this if docker is already installed).    
    We provide a convenience script for this on Ubuntu x86_64. Simply run:    
    `./docker/install_docker_ubuntu.sh`.     
    * After docker installs, enable your user account to run docker without root privileges:   
      `sudo groupadd docker; sudo usermod -aG docker $USER`    
      Then, **log out and log back in** (or restart your computer) for the user/group changes to take effet.
  * Build the docker image:     
    `./docker/build_docker.sh`.     
    This does not build Theseus, but rather only creates a docker image that contains all the necessary dependencies to build and run Theseus. 
  * Run docker image locally as a container:    
    `./docker/run_docker.sh`.    
    Now you can run `make run` or other Theseus-specific build/run commands from within the docker container's shell.

**Notes on Docker usage**    
  * The docker-based workflow should only require you to re-run the `run_docker.sh` script multiple times (e.g., to re-build or run Theseus  modifying its code). The docker installation and docker build steps are one-time only (though it won't hurt to run them again).    
  * KVM doesn't currently work in docker. To run Theseus in QEMU using KVM, you can build Theseus within docker, exit the container, and then run `make orun host=yes` on your host machine.
  * When using docker, you can ignore the following section about installing Rust because our Dockerfile and scripts do it automatically. 


### Installing Rust
*(Not needed if using Docker)*

Install the current Rust compiler and toolchain by following the [setup instructions here](https://www.rust-lang.org/en-US/install.html), which is just this command:    
`curl https://sh.rustup.rs -sSf | sh`

We also need to install Xargo, a drop-in replacement wrapper for Cargo that makes cross-compiling easier:    
`cargo install --vers 0.3.17 xargo`



## Building and Running
To build and run Theseus in QEMU, simply run:   
`make run`

Run `make help` to see other make targets and the various options that can be passed to `make`.


### Note: Rust compiler versions
Because we use the Rust nightly compiler (not stable), the Theseus Makefile checks to make sure that you're using the same version of Rust that we are. We were inspired to add this safety check when we failed to build other Rust projects put out there on Github because they used an earlier version of the nightly Rust compiler than what we had installed on our systems. To avoid this undiagnosable problem, we force you to use a specific version of `rustc` that is known to properly build Theseus; we also use the standard `rust-toolchain` file to ensure this. The Rust version is upgraded as often as possible to align with the latest Rust nightly, but this is a best-effort policy.

As such, if you see a build error about the improper version of `rustc`, follow the instructions displayed after the error message.     


## Using QEMU 
QEMU allows us to run Theseus quickly and easily in its own virtual machine.
To exit Theseus in QEMU, press `Ctrl+Alt+G` (or `Ctrl+Alt` on some systems), which releases your keyboard and mouse focus from the QEMU window. Then press `Ctrl+C` in the terminal window that you ran `make run` from originally to kill QEMU, or you can also quit QEMU using the GUI `(x)` button.

To investigate the hardware/machine state of the running QEMU VM, you can switch to the QEMU console by pressing `Ctrl+Alt+2`. Switch back to the main window with `Ctrl+Alt+1`. On Mac, manually select `VGA` or `compact_monitor0` under `View` from the QEMU menu bar.

### KVM Support
While not strictly required, KVM will speed up the execution of QEMU.
To install KVM, run the following command:    
`sudo apt-get install kvm`.     
To enable KVM support, add `host=yes` to your make command, e.g.,    
`make run host=yes`    
This option is only supported on Linux hosts.


## Debugging 
GDB has built-in support for QEMU, but it doesn't play nicely with OSes that run in long mode. In order to get it working properly with our OS in Rust, we need to patch it and build it locally. The hard part has already been done for us ([details here](https://os.phil-opp.com/set-up-gdb/)), so we can just quickly set it up with the following commands.  

First, install the following packages:  
`sudo apt-get install texinfo flex bison python-dev ncurses-dev`

Then, from the base directory of the Theseus project, run this command to easily download and build it from an existing patched repo:  
`curl -sf https://raw.githubusercontent.com/phil-opp/binutils-gdb/rust-os/build-rust-os-gdb.sh | sh`  

After that, you should have a `rust-os-gdb` directory that contains the `gdb` executables and scripts. 

Then, simply run `make run_pause` to build and run Theseus in QEMU, which will pause the OS's execution until we attach our patched GDB instance.   
To attach the debugger to our paused QEMU instance, run `make gdb` in another terminal. QEMU will be paused until we move the debugger forward, with `n` to step through or `c` to continue running the debugger.  
Try setting a breakpoint at the kernel's entry function using `b nano_core::nano_core_start` or at a specific file/line, e.g., `b scheduler.rs:40`.


## Documentation
Theseus, like the Rust language itself, includes two forms of documentation: the main source-level documentation within the code itself, and a "book" for high-level overviews of design concepts. The latter is a work in progress but may still be useful, while the former is more useful for developing on Thesues.

Once your build environment is fully set up, you can generate Theseus's documentation in standard Rust docs.rs format. 
To do so, simply run:     
`make doc`

To view the documentation in a browser on your local machine, run:     
`make view-doc`

There are similar commands for building the Theseus book:    
`make book`


## IDE Setup  
Our personal preference is to use Visual Studio Code (VS Code), which has excellent, official support from the Rust language team. Other options are available [here](https://areweideyet.com/), but we don't recommend them.

For VS Code, recommended plugins are:
 * Rust (rls), by rust-lang
 * Better TOML, by bungcip
 * x86 and x86_64 Assembly, by 13xforever

### Fixing RLS Problems
Sometimes RLS just doesn't want to behave, especially if the latest Rust nightly version had build errors. In that case, try the following steps to fix it:
 * Set your default Rust toolchain to the one version in the `rust-toolchain` file, for example:     
 `rustup default nightly-2019-07-09`.
 * With your newly-set default toolchain, add the necessary components:    
 `rustup component add rls rust-analysis rust-src`.
 * In VS Code (or whatever IDE you're using), uninstall and reinstall the RLS extension, reloading the IDE each time.
 * Check your IDE's settings to make sure that no weird rust or RLS settings have been set; building Theseus doesn't require any special RLS settings. 
 * If you're still having lots of issues, remove all other toolchains besides the default one and try again.     
 You can see other installed toolchains with `rustup toolchain list`.


# Other

## Booting on Real Hardware
We have tested Theseus on a variety of real machines, including Intel NUC devices, various Thinkpad laptops, and Supermicro servers. 
Currently, the only limiting factor is that the device support booting via USB or PXE using traditional BIOS rather than UEFI; support for UEFI is a work-in-progress. 

To boot over USB, simply run `make boot usb=sdc`, in which `sdc` is the device node for the USB disk itself (*not a partition like sdc2*) to which you want to write the OS image.
On WSL or other host environments where `/dev` device nodes don't exist, you can simply run `make iso` and burn the `.iso` file in the `build/` directory to a USB, e.g., using [Rufus](https://rufus.ie/) on Windows.

To boot Theseus over PXE (network boot), see [this set of separate instructions](book/src/pxe.md).


## Acknowledgements
We would like to express our thanks to the [OS Dev wiki](https://wiki.osdev.org/) and its community and to Philipp Oppermann's [blog_os](https://os.phil-opp.com/) for serving as excellent starting points for Theseus. The early days of Theseus's development progress are indebted to these resources. 


# License
Theseus's source code is licensed under the MIT License. See the LICENSE-MIT file for more. 

# Contributing
We adhere to similar development and style guidelines as the main Rust project.

PRs and issues are welcome from anyone; because Theseus is an experimental OS, certain features may be deprioritized or excluded from the main branch. Don't hesitate to ask or mention something though! :smile:

More details to come.