.DEFAULT_GOAL := all
SHELL := /bin/bash

PWD := $(shell pwd)

KERNEL_BUILD_DIR ?= ${PWD}/../build
CFG_DIR ?= ${PWD}/../../cfg

arch ?= x86_64
## this should not point directly to the .json target spec file, but it should have the same name.
target ?= $(arch)-theseus


MODULE_NAME := $(strip $(shell basename ${PWD}))


.PHONY: all build clean cargo

all: build

build: cargo

clean:
	cargo clean
	rm -rf ${KERNEL_BUILD_DIR}/${MODULE_NAME}.o


cargo: 
	@echo -e "pwd: \"$(PWD)\", module_name: \"${MODULE_NAME}\""
	@mkdir -p $(KERNEL_BUILD_DIR)
	@RUST_TARGET_PATH="${CFG_DIR}" xargo build --release --target $(target)  ## target arg must be a target name, not the file path.
	@cd target/$(target)/release/ && ar x "lib${MODULE_NAME}.rlib" ## extract object file from .rlib file
	@cp -v `find ./target/${target}/release/ -name "${MODULE_NAME}*.o"` $(KERNEL_BUILD_DIR)/__k_${MODULE_NAME}.o

