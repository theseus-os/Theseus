### This Makefile builds only the nano_core object files (as a static library ".a" file), 
### consisting of the nano_core's Rust crate modules and its assembly boot files.
### It does not build an OS .iso image or anything runnable in a simulator like QEMU.

.DEFAULT_GOAL := all
SHELL := /bin/bash

PWD := $(shell pwd)

KERNEL_BUILD_DIR ?= ${PWD}/../build
CFG_DIR ?= ${PWD}/../../cfg
RUSTFLAGS += "-C code-model=large"

BUILD_MODE := debug
# BUILD_MODE := release  ## release mode currently does NOT work! See issue #14
ifeq ($(BUILD_MODE), release)
	XARGO_RELEASE_ARG := --release
endif  ## otherwise, nothing, which is "debug" by default

arch ?= x86_64

## this should not point directly to the .json target spec file, but it should have the same name.
TARGET ?= $(arch)-theseus

## this is the output path of the xargo command, not our choice.
## The name of the lib (lib_____.a) is defined by the target above.
cargo-obj := target/$(TARGET)/$(BUILD_MODE)/libnano_core.a

nano_core-obj := build/nano_core-$(arch).bin

linker_script := src/arch/arch_$(arch)/boot/linker_higher_half.ld
assembly_source_files := $(wildcard src/arch/arch_$(arch)/boot/*.asm)
assembly_object_files := $(patsubst src/arch/arch_$(arch)/boot/%.asm, \
	build/arch/$(arch)/%.o, $(assembly_source_files))


.PHONY: all clean cargo

all: $(nano_core-obj)

clean:
	cargo clean
	rm -rf ./build


cargo: 
#cargo clean   ## temporarily calling cargo clean to fix nano_core makefile
	RUST_TARGET_PATH="${CFG_DIR}" RUSTFLAGS=${RUSTFLAGS} xargo build $(XARGO_RELEASE_ARG) --target $(TARGET)  ## target arg must be a target name, not the file path.


$(nano_core-obj): cargo $(cargo-obj) $(assembly_object_files) $(linker_script)
	@ld -n --gc-sections -T $(linker_script) -o $(nano_core-obj) $(assembly_object_files) $(cargo-obj)	
	@mkdir -p $(KERNEL_BUILD_DIR)
	@cp -vf $(nano_core-obj) $(KERNEL_BUILD_DIR)/nano_core.bin


# compile assembly files
build/arch/$(arch)/%.o: src/arch/arch_$(arch)/boot/%.asm
	@mkdir -p $(shell dirname $@)
	@nasm -felf64 $< -o $@
