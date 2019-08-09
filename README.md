# Theseus OS

Theseus is a new runtime-composable OS that tackles the problem of *state spill*, the harmful yet ubiquitous phenomenon described in our [research paper from EuroSys 2017 here](http://kevinaboos.web.rice.edu/statespy.html).

We have designed and built Theseus from scratch using Rust to completely rethink state management in an OS, with the intention of avoiding state spill or mitigating its effects to the fullest extent possible. 

More details are provided in Theseus's [documentation](#Documentation).



## Building Theseus

Currently, we support building and running Theseus on the following host OSes:
 * Linux, 64-bit Debian-based distributions like Ubuntu, tested on Ubuntu 16.04 and 18.04.
 * Windows, using the Windows Subsystem for Linux (WSL), tested on the Ubuntu version.
 * MacOS, tested on version High Sierra (10.13), but likely works on others. 


### Setting up the build environment

If on Linux or WSL, do the following:
  * Update your system's package list: `sudo apt-get update`.     
  * Install the following packages: `sudo apt-get install nasm pkg-config grub-pc-bin mtools xorriso qemu`     

Additionally, If you're on WSL, you'll need to do the following:
  * Install an X Server for Windows; we suggest using [Xming](https://sourceforge.net/projects/xming/).
  * Set an X display, by running `export DISPLAY=:0`. You'll need to do this each time you open up a new WSL terminal, so it's best to add it to your .bashrc file. You can do that with `echo "export DISPLAY=:0" >> ~/.bashrc`.
  * If you get this error: `Could not initialize SDL(No available video device) - exiting`, then make sure that your X Server is running before running `make run`, and that you have set the `DISPLAY` environment variable above.
  * Install a C compiler and linker toolchain, such as `gcc`.

If you're on Mac OS, do the following:
  * Install [MacPorts](https://www.macports.org/install.php) and [HomeBrew](https://brew.sh/).
  * run the MacOS build setup script: `./scripts/mac_os_build_setup.sh`



### Installing Rust
Install the current Rust compiler and toolchain by following the [setup instructions here](https://www.rust-lang.org/en-US/install.html), which is basically just this command:   
`curl https://sh.rustup.rs -sSf | sh`

We also need to install Xargo, a drop-in replacement wrapper for Cargo that makes cross-compiling easier:    
`cargo install --vers 0.3.13 xargo`



### Building and Running
When you first check out the project, don't forget to checkout all the submodule repositories too:    
`git submodule update --init --recursive`

To build and run Theseus in QEMU, simply run:   
`make run`

Run `make help` to see other make targets. 


#### Note: Rust compiler versions
Because we use the Rust nightly compiler (not stable), the Theseus Makefile checks to make sure that you're using the same version of Rust that we are. We were inspired to add this safety check when we failed to build other Rust projects put out there on Github because they used an earlier version of the nightly Rust compiler than what we had installed on our systems. To avoid this undiagnosable problem, we force you to use a specific version of `rustc` that is known to properly build Theseus. This version is upgraded as often as possible to align with the latest Rust nightly, but this is a best-effort policy.

So, if you see a build error about the improper version of `rustc`, follow the instructions printed out at the end of the error message.     


## Using QEMU 
QEMU allows us to run Theseus quickly and easily in its own virtual machine, completely segregated from the host machine and OS. 
To exit Theseus in QEMU, press `Ctrl+Alt`, which releases your keyboard focus from the QEMU window. Then press `Ctrl+C` in the terminal window that you ran `make run` from originally to kill QEMU. 

To investigate the state of the running QEMU entity, you can switch to the QEMU console by pressing `Ctrl+Alt+2`. Switch back to the main window with `Ctrl+Alt+1`.    


### KVM Support
While not strictly required, KVM will speed up the execution of QEMU.
To install KVM, run the following command:    
`sudo apt-get install kvm`.     
To enable KVM support, add `kvm=yes` to your make command, e.g., `make run kvm=yes`.


## Loading Theseus Through PXE
The following instructions are a combination of [this](https://www.ostechnix.com/how-to-install-pxe-server-on-ubuntu-16-04/) guide on OSTechNix to set up PXE for Ubuntu and [this](https://wellsie.net/p/286/) guide by Andrew Wells for using an arbitrary ISO with PXE.

PXE can be used to load Rust onto a target computer that is connected by LAN to the host machine used for development. To set up the host machine for PXE, first make the Theseus ISO by navigating to the directory Theseus is in and running:   
`make iso`

Then, you will need to set up a TFTP and DHCP server which the test machine will access.
### Setting up the TFTP Server
First, install all necessary packages and dependencies for TFTP:   
`sudo apt-get install apache2 tftpd-hpa inetutils-inetd nasm`   
Edit the tftp-hpa configuration file:   
`sudo nano /etc/default/tftpd-hpa`   
Add the following lines: 
```
RUN_DAEMON="yes"
OPTIONS="-l -s /var/lib/tftpboot"
```
Then, edit the inetd configuration file by opening the editor:   
`sudo nano /etc/inetd.conf`   
And adding:   
`tftp    dgram   udp    wait    root    /usr/sbin/in.tftpd /usr/sbin/in.tftpd -s /var/lib/tftpboot`

Restart the TFTP server and check to see if it's running:   
`sudo systemctl restart tftpd-hpa`  
`sudo systemctl status tftpd-hpa`

If the TFTP server is unable to start and mentions an in-use socket, reopen the tftp-hpa configuration file,set the line that has `TFTP_ADDRESS=":69"` to be equal to `6969` instead and restart the TFTP server. 

### Setting up the DHCP Server
First, install package for DHCP server:   
`sudo apt-get install isc-dhcp-server`

Then run `ifconfig` to view available networking devices and find the network device name, e.g., `eth0`.

Edit the `/etc/default/isc-dhcp-server` configuration file and add the network device name from the step above to "INTERFACES". For me, this looks like `INTERFACES="eth0"`.

Configure an arbitrary IP address that will be used in the next step:   
`sudo ifconfig <network-device-name> 192.168.1.105` 
This command might have to be done each time the computer being used as a server is restarted. 

Edit the `/etc/dhcp/dhcpd.conf` file by uncommenting the line `authoritative;` and adding a subnet configuration such as the one below:
```
subnet 192.168.1.0 netmask 255.255.255.0 {
  range 192.168.1.20 192.168.1.30;
  option routers 192.168.1.1;
  option broadcast-address 192.168.1.255;
  default-lease-time 600;
  max-lease-time 7200;
}

allow booting;
allow bootp;
option option-128 code 128 = string;
option option-129 code 129 = text;
next-server 192.168.1.105;
filename "pxelinux.0";
```

Restart the DHCP server and check to see if it's running:   
`sudo systemctl restart isc-dhcp-server`   
`sudo systemctl status isc-dhcp-server`

### Loading the Theseus ISO Into the TFTP Server
In order for the TFTP server to load Theseus, we need the Theseus ISO and a memdisk file in the boot folder. To get the memdisk file first download syslinux which contains it.   
`wget https://www.kernel.org/pub/linux/utils/boot/syslinux/syslinux-5.10.tar.gz`   
`tar -xzvf syslinux-*.tar.gz`

Then navigate to the memdisk folder and compile.   
`cd syslinux-*/memdisk`   
`make memdisk`   

Next, make a TFTP boot folder for Theseus and copy the memdisk binary into it along with the Theseus ISO:   
`sudo mkdir /var/lib/tftpboot/theseus`   
`sudo cp /root/syslinux-*/memdisk/memdisk /var/lib/tftpboot/theseus/`   
`sudo cp /Theseus/build/theseus-x86_64.iso /var/lib/tftpboot/theseus/`

Navigate to the PXE configuration file:   
`sudo nano /var/lib/tftpboot/pxelinux.cfg/default`   
And add Theseus as a menu option by adding the following:   
```
label theseus
    menu label Theseus
    root (hd0,0)
    kernel theseus/memdisk
    append iso initrd=theseus/theseus-x86_64.iso raw
```
Finally, restart the DHCP server one more time and make sure it's running:   
`sudo systemctl restart isc-dhcp-server`   
`sudo systemctl status isc-dhcp-server`

On the target computer, boot into the BIOS, turn on Legacy boot mode, and select network booting as the top boot option. Once the target computer is restarted, it should boot into a menu which displays booting into Theseus as an option. 

### Subsequent PXE Uses
After setting up PXE the first time, you can run `make pxe` to make an updated ISO, remove the old one, and copy the new one over into the TFTP boot folder. At that point, you should be able to boot that new version of Theseus by restarting the target computer. If there are issues restarting the DHCP server after it worked the first time, one possible solution may be to confirm that the IP address is the one you intended it to be with the command from earlier: 
`sudo ifconfig <network-device-name> 192.168.1.105` 

## Debugging 
GDB has built-in support for QEMU, but it doesn't play nicely with OSes that run in long mode. In order to get it working properly with our OS in Rust, we need to patch it and build it locally. The hard part has already been done for us ([details here](http://os.phil-opp.com/set-up-gdb.html)), so we can just quickly set it up with the following commands.  

First, install the following packages:  
`sudo apt-get install texinfo flex bison python-dev ncurses-dev`

Then, from the base directory of the Theseus project, run this command to easily download and build it from an existing patched repo:  
`curl -sf https://raw.githubusercontent.com/phil-opp/binutils-gdb/rust-os/build-rust-os-gdb.sh | sh`  

After that, you should have a `rust-os-gdb` directory that contains the `gdb` executables and scripts. 

Then, simply run `make debug` to build and run Theseus in QEMU, which will pause the OS's execution until we attach our patched GDB instance.   
To attach the debugger to our paused QEMU instance, run `make gdb` in another terminal. QEMU will be paused until we move the debugger forward, with `n` to step through or `c` to continue running the debugger.  
Try setting a breakpoint at the kernel's entry function using `b nano_core::nano_core_start` or at a specific file/line, e.g., `b scheduler.rs:40`.

## Run Theseus on ARM
Update the submodules. Run:
`git submodule update`

Install QEMU for ARM:
`sudo apt-get install qemu-system-arm`

Install QEMU-EFI firmware
`sudo apt-get install qemu-efi`

Add Grub support for ARM64-efi
You can build the target with grub, or just download the target files. Here is how to build it:
`git clone https://git.savannah.gnu.org/git/grub.git`
`cd grub`
`./autogen.sh` (The command my be different for the newest grub. Just check the INSTALL file)
`mkdir $HOME/grub-dir`
`./configure --prefix=$HOME/grub-dir --target=aarch64 --with-platform=efi`
`make & make install`

Or You can just download the files to a local path:
`mkdir -p $HOME/grub-dir/lib/grub`
`cd $HOME/grub-dir/lib/grub` 
`git clone https://github.com/snoword/arm64-efi.git`

Run Theseus on ARM:
`make arm`

### Debug Theseus on ARM
Downlowd the cross compiler: 
`git clone https://github.com/arter97/aarch64-none-elf-6.1.git`

Add `path_to_aarch64-none-elf/bin` to `$PATH`, or move `path_to_aarch64-none-elf/bin/*` to `/usr/bin` which is already in `$PATH`

Run `make armdebug` in a terminal.
Run `make armgdb` in another terminal.

GDB cannot set breakpoints based on the source file now since we need to generate another file with debug sections for gdb. The reference is here: https://wiki.osdev.org/Debugging_UEFI_applications_with_GDB.

GDB is able to set breakpoints at certain address, check memories and registers or display instructions.




## Documentation
Once your build environment is fully set up, you can generate Theseus's documentation in standard Rust docs.rs format. 
To do so, simply run:     
`make doc`

To view the documentation in a browser on your local machine, run:     
`make view-doc`


## IDE Setup  
Our personal preference is to use Visual Studio Code (VS Code), which has excellent, official support from the Rust language team. Other options are available [here](https://areweideyet.com/), but we don't recommend them.

For VS Code, recommended plugins are:
 * Rust (rls), by rust-lang
 * Better TOML, by bungcip
 * x86 and x86_64 Assembly, by 13xforever


## License
The source code is licensed under the MIT License. See the LICENSE-MIT file for more. 
