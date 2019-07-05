# `cpuio`: Rust wrapper for `inb`, `outb`, etc., instructions

Modified by Kevin Boos, taken from toyos.

**WARNING: The interface to the low-level `outb`, `outw` and `outl`
functions has changed to match Linux.  Please reverse the order of
arguments to these three functions.**

This library is intended to be run on bare metal, and it only depends on
the `core` library.

To use this, add it to your `Cargo.toml` file and call `cpuio::Port::new`
to create a port, being sure to specify `u8`, `u16` or `u32` depending on
the size data supported by the port.

```rust
extern crate cpuio;

use cpuio::Port;

fn main() {
    // Create a port pointing at 0x60, the address of the PS/2 keyboard
    // port on x86 hardware.  This is an unsafe operation because many
    // ports can be used to reconfigure your underlying hardware, and
    // it's the responsiblity of the port creator to make sure it's
    // used safely.
    let mut keyboard: Port<u8> = unsafe { Port::new(0x60) };

    // Read a single scancode from a PS/2 keyboard.  If you run this as
    // an ordinary user, it will fail with a SIGSEGV.
    println!("scancode: {}", keyboard.read());
}
```

The constructor `Port::new` is available as a `const fn`, which allows you
to configure a port at compile time.

The is also an `UnsafePort` type which is identical, except that `read` and
`write` are explicitly marked as unsafe.  It's better to use `UnsafePort`
whenever any individual port operation might corrupt memory or cause
undefined behavior.

## Licensing

Licensed under the [Apache License, Version 2.0][LICENSE-APACHE] or the
[MIT license][LICENSE-MIT], at your option.

[LICENSE-APACHE]: http://www.apache.org/licenses/LICENSE-2.0
[LICENSE-MIT]: http://opensource.org/licenses/MIT
