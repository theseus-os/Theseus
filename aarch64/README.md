### Aarch64+UEFI port of Theseus OS

This is an effort to port Theseus OS to Aarch64 (Arm).

Only basic logging is currently implemented.

This currently targets the "virt" Qemu virtual machine.
It is not tested on any real-life machine at the moment.

### Building and Running

0. You will need

- a Rust toolchain
- `qemu-system-aarch64`
- `wget`
- make
- unzip

1. Build the UEFI app:

```text
$ make build
```

2. Run this app in Qemu:

```text
$ make run
```

3. Once Qemu has started:

    a. Open the "View" menu
    b. Click "Serial" to see the serial output
    c. spam [enter] a few times until you see a prompt
    d. write "fs0:\efi"
    e. You should see the "Hello world" line among the other ones
