# Running Theseus in a Virtual Machine

Using a virtual machine emulator is by far the easiest way to develop, test, and run Theseus. 

## QEMU 
Our primary test environment and recommended emulator is [QEMU](https://www.qemu.org/), which is the default choice for running Theseus using our built-in Makefile commands. 
For example, the `make run` target automatically runs Theseus in a QEMU virtual machine after it finishes the build process. 

The top-level Makefile specifies the configuration parameters for Theseus in QEMU, such as system memory, attached storage devices, serial log output, and more. 
All of these parameters start with `QEMU_` and can be overridden on the command line, or indirectly by setting environment variables such as `net` or `host`, or by editing the Makefile itself.


## Bochs
In older versions of Theseus, we used both [Bochs](https://bochs.sourceforge.io/) and QEMU for testing. Bochs is supported but its configuration may be out of date; the configuration is found in the `bochsrc.txt` ([direct link](https://github.com/theseus-os/Theseus/blob/theseus_main/bochsrc.txt)) file in the root repository directory.

Bochs runs quite slowly and supports virtualization of far fewer hardware devices than QEMU; thus, we do not recommend using it. However, you can try running Theseus in Bochs using the Makefile target for it:
```sh
make bochs
```

## VMware Workstation Player
We have tested Theseus on VMWare Workstation and it generally works out of the box. However, there are some options that you may wish to enable to improve performance and offer access to more devices in Theseus. 

First, [download VMware Workstation Player here](https://www.vmware.com/products/workstation-player/workstation-player-evaluation.html), which can be installed and used for free for non-commercial work. 

On Linux, you will download a `.bundle` file, which then needs to executed in a terminal. For example:
```sh
chmod +x VMware-Player-<...>.bundle
sudo ./VMware-Player-<...>.bundle
```

After opening VMware Workstation Player, do the following:
1. Click `Create A New Virtual Machine`.
2. In the New Virtual Machine Wizard window, choose `Use ISO image:` and browse to select the Theseus ISO image, which should be located in the `build` directory, e.g., `build/theseus-x86_64.iso`. Then, click `Next`.
3. Under `Guest Operating System`, choose the `Other` button and then `Other 64-bit` from the drop-down menu. Then, click `Next`.
4. Set the `Name:` field to "Theseus" or whatever you prefer. Click `Next`.
5. Disk size doesn't matter; click `Next`.
6. Click `Customize Hardware`, and then select the following settings:
    * 512MB of memory (less may work, but the minimum is on the order of 10-20 MB).
    * 2 or more processor cores.
    * Select `Virtualize CPU performance counters` if you want to use them (not required).
    * If you want to obtain Theseus's log output, then you need to add a serial port connection:
        1. Click `Add...` on the bottom left to add a new hardware type, then `Serial Port`.
        2. Under `Connection`, select `Use output file:` and then choose a destination file name for the serial log to be written to. For example, `/home/your_user/theseus_vmware.log`.
        3. Click `Save`. 
7. Click `Finish`, and then `Close`.

Theseus should boot up after a few seconds. You can view the serial log output by `cat`ting or opening the file:
```sh
cat /home/your_user/theseus_vmware.log
```


## VirtualBox
We have tested Theseus on VirtualBox and it generally works out of the box. However, there are some options that you may wish to enable to improve performance and offer access to more devices in Theseus. 

First, [download VirtualBox here](https://www.virtualbox.org/wiki/Downloads) and install it on your system. On Ubuntu and other Debian-based Linux distributions, you will download a `.deb` file that you can open in the Software Installer or install on the command line like so:
```sh
sudo dpkg -i virtualbox-<...>.deb
```

After opening VirtualBox, do the following:
1. Click `New`.
2. In the Create Virtual Machine window, set `Type` to `Other` and `Version` to `Other/Unknown (64-bit)`, choose a name, and then click `Next`. 
3. In the next window, choose 512MB of memory (less may work, but the minimum is on the order of 10-20 MB).
4. Continue clicking next through all of the storage disk options, those do not matter. 
5. Back at the main window, right click on your new Theseus machine and choose `Settings`. 
6. In the left sidebar, click `Storage` and then select the `💿 Empty` option to choose an image for the optical disk.     
  Click on the `💿▾` button on the right side of the `Optical Drive: ` option, select `Choose a disk file`, and then navigate to the Theseus ISO image in the `build/` directory, e.g., `build/theseus-x86_64.iso`.
7. Under `System` in the left sidebar, go to the `Processor` tab and select 2 (or more) processors.
8. If you want to obtain Theseus's log output, then you need to add a serial port connection:    
    1. Click `Serial Ports` in the left sidebar, under the `Port 1` tab, select the `Enable Serial Port` checkbox.
    2. Under the `Port Mode` drop-down menu, select `Raw File` option.
    3. In the `Path/Address` text box, type the destination file name for the serial log to be written to. For example, `/home/your_user/theseus_vbox.log`.
    4. Click `Ok`.
9. In the main window, select the Theseus VM entry from the left sidebar and then click `Start` on the top bar. 

Theseus should boot up after a few seconds. You can view the serial log output by `cat`ting or opening the file:
```sh
cat /home/your_user/theseus_vbox.log
```