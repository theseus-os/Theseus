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
KERNEL_PREFIX ?= k\#
APP_PREFIX    ?= a\#

### NOTE: CURRENTLY FORCING RELEASE MODE UNTIL HASH-BASED SYMBOL RESOLUTION IS WORKING
## Build modes:  debug is default (dev mode), release is release with full optimizations.
## You can set these on the command line like so: "make run BUILD_MODE=release"
# BUILD_MODE ?= debug
BUILD_MODE ?= release

ifeq ($(BUILD_MODE), release)
	CARGO_OPTIONS += --release
endif


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

