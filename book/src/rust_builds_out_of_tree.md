# Building Rust crates out of tree, in a safe way


## Background: The Problem
Because Rust currently [lacks a stable ABI](https://slightknack.github.io/rust-abi-wiki/intro/intro.html), there is no easy, stable, or safe way to integrate two or more separately-compiled Rust binaries together. 
By *integrate*, we mean the ability to have one binary depend upon or invoke another pre-built binary, such as an executable, statically-linked library, dynamically-linked shared object, etc. 

There is another related problem that stems from how the Rust compiler appends unique IDs (metadata used by the compiler) to each compiled crate and each (non-mangled) symbol in those crates; this issue presents itself even in unlinked object files.

As an example, the `page_allocator` crate in Theseus will be compiled into an object file with a name like `page_allocator-c55b593144fe8446.o`, and the function `page_allocator::AllocatedPages::split()` implemented and exposed by that crate will be emitted as the symbol `_ZN14page_allocator14AllocatedPages5split17heb9fd5c4948b3ccfE`.

The values of both the crate's unique ID (`c55b593144fe8446`) and every symbol's unique ID (e.g., `heb9fd5c4948b3ccfE`) are deterministic, but depend on many factors. 
One of those factors is the compiler version, the source directory, the target directory, and more. 
We sometimes refer to both of these unique IDs as a *hash* value since the compiler creates them by hashing together these various factors; how this hash is generated is considered opaque and liable to change, thus we treat it as a black box. 

Theseus loads and links crate object files dynamically at runtime. 
When we build all of the Theseus kernel crates together into a single target directory ([read more here](build_process.md#cargo)),the unique IDs/hash values appended to every crate name and symbol are based on the build machine's source and target directories (among other factors). 
A running instance of Theseus will have a single instance of the `page_allocator` crate loaded into memory and expect all other crates to depend upon that instance, meaning that they should be compiled to expect linkage against its specifically-hashed symbols, e.g., `_ZN14page_allocator14AllocatedPages5split17heb9fd5c4948b3ccfE`.

If you separately compile another crate `my_crate` that depends on the exact same set of Theseus kernel crates, cargo will recompile all Theseus crates *from source* into that new target directory, resulting in the recompiled object files and their symbols having completely different unique ID hashes from the original Theseus instance. 
As such, when attempting to load `my_crate` into that already-running prebuilt instance of Theseus, it will fail to load and link because that version of `my_crate` will depend on differently-hashed crates/symbols, e.g., it may depend upon the symbol `_ZN14page_allocator14AllocatedPages5splithd64cba3bd66ea729E` instead of `_ZN14page_allocator14AllocatedPages5split17heb9fd5c4948b3ccfE` (note the different appended hash values).

Therefore, the *real* problem is that there is no default, easy way to tell cargo that it should build a crate against a prebuilt set of dependencies. [See this GitHub issue for more](https://github.com/rust-lang/cargo/issues/1139) about why this feature would be useful, but why it still isn't supported (hint: no stable Rust ABI).


### Bad solution

Technically, we can solve this by using an existing non-Rust stable ABI, i.e., the C language ABI. 
However, doing so requires defining/exposing Rust functions, data, types, or any other kind of symbol in a C-compatible way, such that you can invoke them using the C ABI (its expected struct memory layout and calling convention).

Unfortunately, this necessitates the usage of unsafe FFI code blocks (via C-style extern functions) to connect two separate bodies of fully-safe Rust code, which is just plain dumb, not to mention tedious!
In the above example, we would be required to export the `page_allocator::AllocatePages::split()` function in the `page_allocator` crate as such:
```rust
#[no_mangle]
pub extern "C" fn split_allocated_pages(...) { ... }
```
and then invoke it unsafely from the dependent `my_crate` as such:
```rust
extern "C" {
    fn split_allocated_pages(...) { ... }
}
fn foo(ap: AllocatedPages) {
    unsafe {
        split_allocated_pages(ap);
        ...
    }
}
```
instead of just invoking `AllocatedPages::split()` directly in safe Rust code. (Note that many FFI details are omitted above.)


Surely we can do better!


## Solution: `theseus_cargo` for out-of-tree builds

The solution is to force cargo to use the existing pre-built crate objects to resolve dependencies that an out-of-tree crate has on Theseus's in-tree crates, and prevent cargo from re-compiling all of Theseus's in-tree crates from source.

This is realized in two parts:
1. Generating the prebuilt dependencies while building Theseus, which will be used to resolve dependencies when separately building the out-of-tree crate(s),
2. Correctly building the out-of-tree crate(s) against those prebuilt Theseus dependencies. 


### 1. Generating the set of prebuilt dependencies

To create a set of dependency files understood by Rust's compiler toolchain, the main top-level Makefile  we build all of the 

We use the `tools/copy_latest_crate_objects` build tool to accomplish this:
```mk
cargo run --release --manifest-path $(ROOT_DIR)/tools/copy_latest_crate_objects/Cargo.toml -- \
    -i ./target/$(TARGET)/$(BUILD_MODE)/deps \
    --output-objects $(OBJECT_FILES_BUILD_DIR) \
    --output-deps $(DEPS_DIR) \
    --output-sysroot $(DEPS_SYSROOT_DIR)/lib/rustlib/$(TARGET)/lib \
    -k ./kernel \
    -a ./applications \
    --kernel-prefix $(KERNEL_PREFIX) \
    --app-prefix $(APP_PREFIX) \
    -e "$(EXTRA_APP_CRATE_NAMES) libtheseus"
```


### 2. Building other Rust code against the prebuilt Theseus dependencies

TODO: describe `theseus_cargo` build tool and what it does.

See the [`tools/theseus_cargo` source code](https://github.com/theseus-os/Theseus/blob/theseus_main/tools/theseus_cargo/src/main.rs) for more details.






## Related Links, Discussions, Alternative Approaches

* [A Stable Modular ABI for Rust (Rust Internals Forum)](https://internals.rust-lang.org/t/a-stable-modular-abi-for-rust/12347/69)
* [The Rust ABI wiki](https://slightknack.github.io/rust-abi-wiki/)
* The [abi_stable](https://crates.io/crates/abi_stable) crate, which provides "safe" traits, macros, and wrappers around underlying Rust-to-Rust FFI.