### This makefile is run from each kernel crate, so it needs to use paths
### relative to a subdirectory, not the actual directory contaiing this file.
### So, to access the directory containing this file, you would use "../"

.DEFAULT_GOAL := all
SHELL := /bin/bash

ARCH ?= x86_64

## this should not point directly to the .json target spec file, but it should have the same name.
TARGET ?= $(ARCH)-theseus

PWD := $(shell pwd)

# KERNEL_BUILD_DIR ?= ${PWD}/../build
KERNEL_BUILD_DIR ?= ${PWD}/build

## specifies where the configuration files are kept, like target json files
# CFG_DIR ?= ${PWD}/../../cfg
CFG_DIR ?= ${PWD}/../cfg
#BUILD_MODE := debug
BUILD_MODE := release
ifeq ($(BUILD_MODE), release)
	XARGO_RELEASE_ARG := --release
endif  ## otherwise, nothing, which is "debug" by defaul

## emit obj gives us the object file for the crate, instead of an rlib that we have to unpack.
RUSTFLAGS += --emit=obj
## enable debug info even for release builds
RUSTFLAGS += -C debuginfo=2
## using a large code model 
RUSTFLAGS += -C code-model=large
## promote unused must-use types (like Result) to an error
RUSTFLAGS += -D unused-must-use

