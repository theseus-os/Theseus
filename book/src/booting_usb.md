# Booting Theseus from a USB drive

To boot over USB, simply run
```sh
make boot usb=sdc
```
in which `sdc` is the device node for the USB disk itself *(**not a partition** like sdc2)*.
The OS image (.iso file) will be written to that USB drive.

On WSL or other host environments where `/dev` device nodes don't exist, you can simply run `make iso` and burn the `.iso` file in the `build/` directory to a USB drive. 
For example, on Windows we recommend using [Rufus](https://rufus.ie/) to burn ISOs.

Then, once the bootable USB is ready, plug it into your PC, restart or power on the machine, and choose that USB device from the BIOS or legacy boot device screen.