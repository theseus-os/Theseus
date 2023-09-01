### This makefile is used for any crate that runs in the kernel (kernel/ and applications/)
### so it needs to use paths relative to a subdirectory, not the actual directory contaiing this file.
### So, to access the directory containing this file, you would use "../"

.DEFAULT_GOAL := all
SHELL := /bin/bash

## specifies which architecture we're building for
ARCH ?= x86_64

## The filename and commit hash of the EFI firmware to fetch from
## https://github.com/retrage/edk2-nightly/.
ifeq ($(ARCH),x86_64)
	OVMF_COMMIT = 7ca5064968a54d84831e5785aea87cb9c71d4a3d
	OVMF_FILE ?= RELEASEX64_OVMF.fd
else ifeq ($(ARCH),aarch64)
	OVMF_COMMIT = 7706abd0defa07249946e5908a5dc84c4c5a7d44
	OVMF_FILE ?= RELEASEAARCH64_QEMU_EFI.fd
else
$(error 'ARCH' '$(ARCH)' is invalid; we only support 'x86_64' and 'aarch64')
endif

## The QEMU binary to run.
QEMU_BIN = qemu-system-$(ARCH)

## The name of the target JSON file (without the ".json" suffix)
TARGET ?= $(ARCH)-unknown-theseus

## The top level directory of the Theseus project
ROOT_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/..)

## Where the configuration files are kept, like target json files
CFG_DIR := $(ROOT_DIR)/cfg

## Prefixes for object files
KERNEL_PREFIX       ?= k\#
APP_PREFIX          ?= a\#
EXECUTABLE_PREFIX   ?= e\#

## By default, the Makefile will build the entire Theseus workspace
## as specified by the `members` set in the top-level `Cargo.toml` file
## (excluding the crates that are part of the `exclude` set).
## However, one can override which crates are built by setting `FEATURES`.
FEATURES ?= --workspace

## Currently, we cannot build all crates in the workspace on aarch64, just the minimum subset.
ifeq ($(ARCH),aarch64)
	FEATURES =
endif

## Build modes: debug is development mode, release is with full optimizations.
## We build using release mode by default, because running in debug mode is quite slow.
## You can set these on the command line like so: "make run BUILD_MODE=release"
BUILD_MODE ?= release
ifeq ($(BUILD_MODE),debug)
    ## "debug" builds are the default in cargo, so don't change cargo options. 
    ## However, we do define the DEBUG value for CFLAGS, which is used in the assembly boot code.
	export override CFLAGS+=-DDEBUG
else ifeq ($(BUILD_MODE),release)
	## "release" builds require passing the `--release` flag, but it can only be passed once.
	ifeq (,$(findstring --release,$(CARGOFLAGS)))
		export override CARGOFLAGS+=--release
	endif
else 
$(error 'BUILD_MODE' value of '$(BUILD_MODE)' is invalid, it must be either 'debug' or 'release')
endif


## Tell cargo to build our own target-specific version of the `core` and `alloc` crates.
## Also ensure that core memory functions (e.g., memcpy) are included in the build and not name-mangled.
## We keep these flags separate from the regular CARGOFLAGS for purposes of easily creating a sysroot directory.
BUILD_STD_CARGOFLAGS += -Z unstable-options
BUILD_STD_CARGOFLAGS += -Z build-std=core,alloc
BUILD_STD_CARGOFLAGS += -Z build-std-features=compiler-builtins-mem


## Tell rustc to output the native object file for each crate,
## which avoids always having to unpack the crate's .rlib archive to extract the object files within.
## Note that we still do have to extract and partially link object files from .rlib archives for crates that
## use a build script to generate additional object files during build time.
export override RUSTFLAGS += --emit=obj
## enable debug info even for release builds
export override RUSTFLAGS += -C debuginfo=2
## promote unused must-use types (like Result) to an error
export override RUSTFLAGS += -D unused-must-use

## This prevents monomorphized instances of generic functions from being shared across crates.
## It vastly simplifies the procedure of finding missing symbols in the crate loader,
## because we know that instances of generic functions will not be found in another crate
## besides the current crate or the crate that defines the function.
## As far as I can tell, this does not have a significant impact on object code size or performance.
## More info: <https://internals.rust-lang.org/t/explicit-monomorphization-for-compilation-time-reduction/15907/7>
##
## Update: this might work now, see <https://github.com/rust-lang/rust/issues/96486#issuecomment-1180395751>.
## Thus, we should experiment with removing this to see if it offers any code size reduction benefits
## without breaking our loader/linker assumptions.
export override RUSTFLAGS += -Z share-generics=no

## This forces frame pointers to be generated, i.e., the stack base pointer (RBP register on x86_64)
## will be used to store the starting address of the current stack frame.
## This can be used for obtaining a backtrace/stack trace,
## but isn't necessary because Theseus supports parsing DWARF .debug_* sections to handle stack unwinding.
## Note that this reduces the number of registers available to the compiler (by reserving RBP),
## so it often results in slightly lowered performance. 
## By default, this is not enabled.
# export override RUSTFLAGS += -C force-frame-pointers=yes
