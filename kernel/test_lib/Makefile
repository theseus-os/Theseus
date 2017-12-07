.DEFAULT_GOAL := all

KERNEL_BUILD_DIR ?= build

arch ?= x86_64

## this should not point directly to the .json target spec file, but it should have the same name.
target ?= $(arch)-theseus


.PHONY: all build clean cargo

all: build

build: cargo

clean:
	@cargo clean
	@rm -rf build


cargo: 
	@mkdir -p ../$(KERNEL_BUILD_DIR)
	@RUST_TARGET_PATH="../../cfg" xargo build --release --target $(target)  ## target arg must be a target name, not the file path.
	@cp target/$(target)/release/libtest_lib.rlib ../$(KERNEL_BUILD_DIR)

