Managed
=======

_managed_ is a library that provides a way to logically own objects, whether or not
heap allocation is available.

Motivation
----------

The _managed_ library exists at the intersection of three concepts: _heap-less environments_,
_collections_ and _generic code_. Consider this struct representing a network interface:

```rust
pub struct Interface<'a, 'b: 'a,
    DeviceT:        Device,
    ProtocolAddrsT: BorrowMut<[IpAddress]>,
    SocketsT:       BorrowMut<[Socket<'a, 'b>]>
> {
    device:         DeviceT,
    hardware_addr:  EthernetAddress,
    protocol_addrs: ProtocolAddrsT,
    sockets:        SocketsT,
    phantom:        PhantomData<Socket<'a, 'b>>
}
```

There are three things the struct `Interface` is parameterized over:
  * an object implementing the trait `DeviceT`, which it owns;
  * a slice of `IPAddress`es, which it either owns or borrows mutably;
  * a slice of `Socket`s, which it either owns or borrows mutably, and which further either
    own or borrow some memory.

The motivation for using `BorrowMut` is that in environments with heap, the struct ought to
own a `Vec`; on the other hand, without heap there is neither `Vec` nor `Box`, and it is only
possible to use a `&mut`. Both of these implement BorrowMut.

Note that owning a `BorrowMut` in this way does not hide the concrete type inside `BorrowMut`;
if the slice is backed by a `Vec` then the `Vec` may still be resized by external code,
although not the implementation of `Interface`.

In isolation, this struct is easy to use. However, when combined with another codebase, perhaps
embedded in a scheduler, problems arise. The type parameters have to go somewhere! There
are two choices:
  * either the type parameters, whole lot of them, infect the scheduler and push ownership
    even higher in the call stack (self-mutably-borrowing structs are not usable in safe Rust,
    so the scheduler could not easily own the slices);
  * or the interface is owned as a boxed trait object, excluding heap-less systems.

Clearly, both options are unsatisfying. Enter _managed_!

Installation
------------

To use the _managed_ library in your project, add the following to `Cargo.toml`:

```toml
[dependencies]
managed = "0.1"
```

The default configuration assumes a hosted environment, for ease of evaluation.
You probably want to disable default features and configure them one by one:

```toml
[dependencies]
managed = { version = "...", default-features = false, features = ["..."] }
```

### Feature `std`

The `std` feature enables use of `Box` and `Vec` through a dependency on the `std` crate.

### Feature `alloc`

The `alloc` feature enables use of `Box` through a dependency on the `alloc` crate.
This only works on nightly rustc.

### Feature `collections`

The `collections` feature enables use of `Vec` through a dependency on
the `collections` crate. This only works on nightly rustc.

Usage
-----

_managed_ is an interoperability crate: it does not include complex functionality but rather
defines an interface that may be used by many downstream crates. It includes two enums:

```rust
pub enum Managed<'a, T: 'a + ?Sized> {
    Borrowed(&'a mut T),
    #[cfg(/* Box available */)]
    Owned(Box<T>),
}

pub enum ManagedSlice<'a, T: 'a> {
    Borrow(&'a mut [T]),
    #[cfg(/* Vec available */)]
    Owned(Vec<T>)
}
```

The enums have the `From` implementations from the corresponding types, and `Deref`/`DerefMut`
implementations to the type `T`, as well as other helper methods; see the [full documentation][doc]
for details.

Of course, the enums can be always matched explicitly as well.

[doc]: https://docs.rs/managed/

License
-------

_managed_ is distributed under the terms of 0-clause BSD license.

See [LICENSE-0BSD](LICENSE-0BSD.txt) for details.
