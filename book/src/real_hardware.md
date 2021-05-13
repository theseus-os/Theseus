# Running Theseus on Virtual or Real Hardware

We have tested whether Theseus runs properly in a variety of environments, currently on x86_64 only:
* Virtual machine emulators: QEMU, bochs, VirtualBox, VMware Workstation Player.
* Real hardware: Intel NUC devices, Supermicro servers, various Thinkpad laptops, PCs with Gigabyte motherboards.

Currently, the primary limiting factor is that the device support booting via USB or PXE using traditional BIOS rather than UEFI; support for UEFI is a work-in-progress. 

Note that as Theseus is not fully mature, booting on your own hardware is done at your own risk. Be sure that you have a backup of all of your important files before doing so. 

If you experience a problem booting Theseus on any virtual or real hardware platform, please take a look at [the open issues on GitHub](https://github.com/theseus-os/Theseus/issues/) to see if someone has already reported your problem or attempted to fix it. 
If so, leave a comment describing your experience or open a new issue to help the Theseus developers work towards supporting your hardware environment!
