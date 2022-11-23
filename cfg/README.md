## Theseus OS target specifications

Theseus OS uses its own target specification to configure how `rustc` compiles its code.
The default target spec is [x86_64-unknown-theseus.json], which builds Theseus for the `x86_64` architecture.
If you're unfamiliar with [target triples] or [how Rust handles targets] or how Rust [supports platform cfg via targets], here's a quick breakdown:
* `x86_64`: the architecture.
  * A "sub-architecture" can also be provided, but we do not use this on x86,
    as it typically only applies to ARM architectures.
* `unknown`: the vendor.
  * Theseus doesn't have a specific vendor; the default value is `unknown`.
* `theseus`: the operating system.
  * Optionally, the OS parameter can be appended with other items, such as the system environment or ABI.
    * Theseus doesn't yet specify an environment or ABI,
      but our `llvm-target` item selects `elf`.

We describe the key items in the target spec below; you can read more about the various options
in [`rustc`'s `TargetSpec` type documentation](https://docs.rs/rustc-ap-rustc_target/latest/rustc_ap_rustc_target/spec/struct.TargetOptions.html).

* `llvm-target`: all Theseus targets are based on `x86_64-unknown-none-elf`,
  which is a minimal target that specifies no underlying OS and
  uses the ELF file format for compiled artifacts.
* `features`: `-mmx,-sse,+soft-float`.
  This builds Theseus with hardware floating point support disabled in favor of soft floating point.
  This is the typical choice for most OS kernels, since using hardware floating point and/or SIMD
  instructions causes more overhead during a context switch, as all of the actively-used SIMD
  registers must also be saved to and restored from the stack.

* [`code-model`]: we use the `large` code model because Theseus runs in a single address space,
  meaning that code may exist at addresses across the entire 48-bit address space.
  Thus, an instruction that jumps to or references another address must be able to
  access addresses anywhere in the address space.

* [`relocation-model`]: we use the `static` relocation model to keep the logic of our
  runtime loader and linker as simple as possible.
   * This relocation model avoids GOT- and PLT-based relocation entries in favor of
     direct relocation models based on absolute addresses.
   * In the future, we may support other relocation models, but for now this is required.

* [`tls-model`]: we use the `local-exec` model for Thread-Local Storage (TLS) because
  it is the simplest and most efficient model.
  Also, Theseus's runtime loader/linker can even support the `local-exec` TLS model
  in crate object files that are dynamically loaded during runtime, even if they weren't
  included in the initial build-time list of TLS sections that exist in the
  statically-linked base kernel image.
  * Currently, Theseus doesn't support the other three TLS models, as some of them
    (e.g., `initial-exec`) require support for a Global Offset Table (GOT).

* [`merge-functions`]: we disable this option in order to ensure that `loadable` mode
  works correctly, in which Theseus loads and links all crate object files at runtime.
  Without this, some functions may be merged together, preventing our loader/linker from
  finding generic function implementations in the expected emitted object files.
  See [PR #57268](https://github.com/rust-lang/rust/pull/57268) and
  [Issue #57356](https://github.com/rust-lang/rust/issues/57356) for more details.
  * It appears that Theseus still works correctly without setting this option,
    so we may not need it, but it doesn't hurt to explicitly disable it.

### Other target specs
* [x86_64-unknown-theseus-sse.json]: similar to the default `x86_64-unknown-theseus`, but enables the compiler to
  generate instructions that use SSE2 (and lower SSE versions) SIMD features.
  With this, all crates across Theseus can use SSE2 SIMD instructions and registers.
* [x86_64-unknown-theseus-avx.json]: similar to above, but enables AVX (version 1) instructions/registers.
  With this, all crates across Theseus can use AVX SIMD instructions and registers.
  This does not yet enable support for AVX2 or AVX512.


[x86_64-unknown-theseus.json]: ./x86_64-unknown-theseus.json
[x86_64-unknown-theseus-sse.json]: ./x86_64-unknown-theseus-sse.json
[x86_64-unknown-theseus-avx.json]: ./x86_64-unknown-theseus-avx.json
[`code-model`]: https://doc.rust-lang.org/rustc/codegen-options/index.html#code-model
[`relocation-model`]: https://doc.rust-lang.org/rustc/codegen-options/index.html#relocation-model
[`tls-model`]: https://doc.rust-lang.org/beta/unstable-book/compiler-flags/tls-model.html#tls_model
[target triples]: https://clang.llvm.org/docs/CrossCompilation.html#target-triple
[how Rust handles targets]: https://doc.rust-lang.org/rustc/targets/index.html
[supports platform cfg via targets]: https://doc.rust-lang.org/nightly/rustc/platform-support.html