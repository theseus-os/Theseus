# Running Theseus on Headless Systems Interactively

By default, Theseus expects to run in a standard desktop environment with a basic graphical display (monitor) and a real keyboard (and optionally, a mouse). 
For interacting with the system, Theseus uses the keyboard as its primary input and the graphical display as its primary output.

Theseus can also run in [headless mode](https://en.wikipedia.org/wiki/Headless_computer), in which the "head" of the computer (its monitor and keyboard) are nonexistent, and I/O is done over the network or a serial port connection.
This is useful for system environments like servers, embedded microcontrollers, certain virtual machine environments, and anything else without a monitor display.

The current version of Theseus listens for incoming connections on serial ports only (COM1 and COM2).
Upon receiving data (like a keypress) on a serial port, Theseus will spawn a terminal emulator to handle I/O on that port. 

> Note: headless interactive mode can coexist simultaneously with regular graphical display mode.

TODO: describe the various options for disabling hardware graphical displays

## Connecting a Terminal Emulator to Theseus
Currently, we have tested this with only virtual serial ports connected to Theseus through a VMM like QEMU, but it should also work for real hardware serial ports.

While Theseus is running in a VM or on another physical machine, we recommend using a terminal emulator program on the host machine to connect to the serial port device on the Theseus machine.
Examples include:
  * `screen`
  * `picocom`
  * `minicom`

By default, the Theseus Makefile starts QEMU with two serial ports, COM1 and COM2, that the host machine can connect to and exchange data over. 
The first serial port, COM1, is connected to the `stdio` streams of the terminal that spawned the QEMU process. 
This stream is used for the system log and for controlling QEMU, so it is best to use COM2 to interact with Theseus separately such that the headless virtual terminal is not polluted with log statements.
The second serial port, COM2, is connected to a dynamically-allocated pseudo-terminal (PTY) that will be allocated by QEMU. To connect to it, inspect the QEMU output when it first starts to find a line like this:
```
char device redirected to /dev/pts/3 (label serial1-base)
```
This tells you that QEMU connected the second serial port (COM2) on the guest Theseus VM to the Linux PTY device at `/dev/pts/3`. 
Note that QEMU uses 0-based indexing for serial ports, so its "serial1" label refers to the second serial port, our "SERIAL2" (COM2 on x86).

Now, once QEMU is running, you can connect a terminal emulator on the host to the serial port in Theseus, and Theseus will issue interactive commands to that terminal emulator.
To do so, run any of the following commands and then press any key to start the terminal prompt:
  * `screen /dev/pts/3`
  * `picocom /dev/pts/3`
  * `minicom -D /dev/pts/3`

Note that some programs (namely `minicom`) do not necessarily send the expected value when pressing the `Backspace` key. 
Thus, if you are experiencing unexpected behavior when pressing `Backspace`, you need to ensure that your program is sending the correct ASCII `DEL` (0x7F) character value when pressing `Backspace`, instead of an ASCII `BS` (0x08), which will only move the cursor to the left by one character.
In our experience, `screen` and `picocom` work as expected, but `minicom` does not. 
To change `minicom`'s default behavior, you can do the following:
  * Press `Ctrl + A` twice to open the meta control bar at the bottom of the screen
  * Press `T` to open the "Terminal Settings" menu
  * Press `B` to toggle the "Backspace key sends" setting.
    - You want this to be `DEL`, not `BS`.

Either of these serial ports can be changed in QEMU using the environment variables `SERIAL1` and `SERIAL2` respectively, though again, we recommend only using `SERIAL2` in virtual environments.
In real hardware, where there is only one serial port and therefore COM1 must be used, you can disable the log or initialize it with a different serial port, e.g., COM2, to avoid polluting the terminal emulator connected to COM1 with system log printouts.
