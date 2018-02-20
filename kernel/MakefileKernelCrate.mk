.DEFAULT_GOAL := all
SHELL := /bin/bash

PWD := $(shell pwd)

KERNEL_BUILD_DIR ?= ${PWD}/../build

## specifies where the configuration files are kept, like target json files
CFG_DIR ?= ${PWD}/../../cfg

arch ?= x86_64

## the name of the target json file
target ?= $(arch)-theseus

## the module name is the name of the directory 
MODULE_NAME := $(strip $(shell basename ${PWD}))

## emit obj gives us the object file for each crate, instead of an rlib that we have to unpack.
RUSTFLAGS += --emit=obj

## using a large code model 
RUSTFLAGS += -C code-model=large

## promote unused must-use types (like Result) to an error
RUSTFLAGS += -D unused-must-use

.PHONY: all clean cargo

all: cargo

clean:
	cargo clean
	rm -rf ${KERNEL_BUILD_DIR}/__k_${MODULE_NAME}.o


## Builds the crate, and then copies all object files to its own module-specific subdirectory in the kernel build directory
cargo: 
	@mkdir -p $(KERNEL_BUILD_DIR)/${MODULE_NAME}
	@RUST_TARGET_PATH="${CFG_DIR}" RUSTFLAGS="${RUSTFLAGS}" xargo build --release --verbose --target $(target)
	@for objfile in ./target/$(target)/release/deps/*.o ; do \
		cp -vf $${objfile}  $(KERNEL_BUILD_DIR)/${MODULE_NAME}/`basename $${objfile} | sed -n -e 's/\(.*\)-.*.o/\1.o/p'` ; \
	done

#
