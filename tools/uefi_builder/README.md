Using different crates for different targets is necessary until [rust-lang/
cargo/10030][1] is implemented.

If you encounter:
```
error: no matching package named `bootloader` found
````
Try compiling with `-Z bindeps`.

[1]: https://github.com/rust-lang/cargo/issues/10030
