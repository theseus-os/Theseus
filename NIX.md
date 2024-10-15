# Building and Running Theseus with Nix

Theseus provides a Nix shell ([shell.nix](./shell.nix)) that provides a build environment for building, developing, and running Theseus in QEMU. It can bring in all the dependencies needed, as described in the [README](./README.md), including the correct build of Rust nightly (see [Theseus, Nix, and Rust Toolchains](#theseus_nix_and_rust_toolchains)). The shell also includes some optional arguments you can use to optimize the dependency imports as well as other things. See [Nix Shell](#nix_shell) for more.

## Quick Start

To enter the Nix shell, simply use `nix-shell` while in the Theseus source.

```sh
nix-shell # [./shell.nix]
```

From here you should be able to build, run, and develop any part of Theseus:

```sh
# Equivilant to `make iso` but builds with all features enabled. See Makefile.
make
```

```sh
make iso
```

```sh
# Build and run Theseus with full features in QEMU.
make run
```

## Nix Shell

### Args

The following are some of the more relevant arguments for the shell. Some additional arguments exist, but are only listed and documented in [shell.nix](./shell.nix) itself. They are omitted here for the sake of not cluttering this documentation for less advanced Nix users.

#### Dependency-Related Options

- `run` - Whether or not to prepare for running Theseus in QEMU. This can be highly useful when you don't plan to run Theseus in QEMU, as QEMU is a very heavy dependency. The default is `true`.
- `extraRustDevelopmentTools` - Brings additional development-related tools, not in the standard Theseus build toolchain, into the provided toolchain. Currently, this only includes rust-analyzer. The default is `true`.
- `extraRustToolchainComponents` - Extra, custom components to add to the provided toolchain.
- `rustToolchain` - Override with a completely custom Rust toolcahin derivation. Not recommended.
- `bootloader` - Chooses which bootloader(s) the shell should prepare for usage.
  Possible values are:
    - "any" - All bootloaders are made available to use, and the `bootloader` env var is not set.
    - "grub" - GRUB's dependancies are added, and the `bootloader` env var is set to
      "grub" if `setMakeBootloaderPreference` is `true`.
    - "limine" - The shell preforms the setup stated in the README, cloning limine and resetting
      to the needed commit. Additionally, the `bootloader` env var is set to "limine" if
      `setMakeBootloaderPreference` is `true`.
  The default is `"grub"`.
  - `setMakeBootloaderPreference` - If false, overrides the behavior of `bootloader` to _not_
    set the `bootloader` environment variable. The default is `true`.

#### Limine-Related Options

These options are specific to the usage of the Limine bootloader. If the above arguments do not create a shell in which Limine is set as a dependency, these options have no effect. See [README.md](./README.md) about Limine and its usage in Theseus.

- `preserveOldLimine` - Before downloading Limine, check if './limine-prebuilt' already exists. If so, skip downloading Limine for the already-existing Limine to be used. The default is `true`.
- `requiredLimineCommit` - If Limine is downloaded, this specifies the commit that Limine should be checked out to. The default is what is specified in README.md and changing this is not recommeded. It is merely set as an arg rather than a constant for convenience.

### Examples

Default shell (with QEMU for running, and both GRUB and Limine availbale to use):

```sh
nix-shell
```

Create build/dev environment without QEMU dependency (avoids downloading qemu_kvm but disables running):

```sh
nix-shell --arg run 'false'
```

Use the Limine bootloader (will fully set up for Limine usage):

```sh
nix-shell --arg bootloader '"limine"'
# Should now use Limine by default:
make run
```

### Targetting Other Architectures

TODO

### Development Shell for Debugging

TODO

## Theseus, Nix, and Rust Toolchains

Theseus's Rust codebase and the locked versions of its dependencies rely on not only nightly Rust, but a specific nightly build. This is not an issue for systems using rustup, however, Nix's package manager is at its core and not only is using something like rustup looked down on, but it may not even work on NixOS. Normally, when Theseus is built using cargo, it will see the rust-toolchain.toml file and use rustup to install that toolchain to use instead. Nix, however, only provides the latest _stable_ Rust in Nixpkgs, and the wrapped toolchain components that come with it do not do this. This is an obvious problem.  

Luckily, though, there is an easy solution: [Oxalica's rust-overlay](https://github.com/oxalica/rust-overlay/tree/master). Using this, we can easily produce a working, NixOS-compatible Rust toolchain from a Rust toolchain file, such as the one that provides the toolchain definition for Theseus, ([rust-toolchain.toml](./rust-toolchain.toml)).

This toolchain is provided by the [rust-toolchain.nix](./rust-toolchain.nix) file. You can import it directly to produce a Rust toolchain for Theseus but its main use is in the Theseus Nix shell. 

## Nix Derivation for Theseus

A derivation for Theseus is in the works.

## Flake Support

TODO
