### This makefile is used for any crate that runs in the kernel (kernel/ and applications/)
### so it needs to use paths relative to a subdirectory, not the actual directory contaiing this file.
### So, to access the directory containing this file, you would use "../"

.DEFAULT_GOAL := all
SHELL := /bin/bash

## specifies which architecture we're building for
ARCH ?= x86_64

## The name of the target JSON file (without the ".json" suffix)
TARGET ?= $(ARCH)-theseus

## The top level directory of the Theseus project
ROOT_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/..)

## Where the configuration files are kept, like target json files
CFG_DIR := $(ROOT_DIR)/cfg

## Prefixes for object files
KERNEL_PREFIX       ?= k\#
APP_PREFIX          ?= a\#
EXECUTABLE_PREFIX   ?= e\#


## Build modes: debug is development mode, release is with full optimizations.
## We build using release mode by default, because running in debug mode is quite slow.
## You can set these on the command line like so: "make run BUILD_MODE=release"
BUILD_MODE ?= release
CARGOFLAGS ?=
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


## emit obj gives us the object file for the crate, instead of an rlib that we have to unpack.
RUSTFLAGS += --emit=obj
## enable debug info even for release builds
RUSTFLAGS += -C debuginfo=2
## using a large code model 
RUSTFLAGS += -C code-model=large
## use static relocation model to avoid GOT-based relocation types and .got/.got.plt sections
RUSTFLAGS += -C relocation-model=static
## promote unused must-use types (like Result) to an error
RUSTFLAGS += -D unused-must-use

## As of Dec 31, 2018, this is needed to make loadable mode work, because otherwise, 
## some core generic function implementations won't exist in the object files.
## Details here: https://github.com/rust-lang/rust/pull/57268
## Relevant rusct commit: https://github.com/jethrogb/rust/commit/71990226564e9fe327bc9ea969f9d25e8c6b58ed#diff-8ad3595966bf31a87e30e1c585628363R8
## Either "trampolines" or "disabled" works here, not sure how they're different
RUSTFLAGS += -Z merge-functions=disabled
# RUSTFLAGS += -Z merge-functions=trampolines

## This prevents monomorphized instances of generic functions from being shared across crates.
## It vastly simplifies the procedure of finding missing symbols in the crate loader,
## because we know that instances of generic functions will not be found in another crate
## besides the current crate or the crate that defines the function.
## As far as I can tell, this does not have a significant impact on object code size or performance.
RUSTFLAGS += -Z share-generics=no

## This forces frame pointers to be generated, i.e., the stack base pointer (RBP register on x86_64)
## will be used to store the starting address of the current stack frame.
## This can be used for obtaining a backtrace/stack trace,
## but isn't necessary because Theseus supports parsing DWARF .debug_* sections to handle stack unwinding.
## Note that this reduces the number of registers available to the compiler (by reserving RBP),
## so it often results in slightly lowered performance. 
## By default, this is not enabled.
# RUSTFLAGS += -C force-frame-pointers=yes



## TODO: Remove this later once we adress the various new rustc warnings
RUSTFLAGS += -Aunsupported_naked_functions
RUSTFLAGS += -Arenamed_and_removed_lints
RUSTFLAGS += -Adeprecated
