### This Makefile builds the specified subdirectories within this kernel directory.
### It offers the "all" and "clean" targets only
SHELL := /bin/bash


KERNEL_BUILD_DIR ?= $(shell pwd)/build
export  # export this to Makefiles in subdirectories

# get all the subdirectories for making
SUBDIRS := $(wildcard */.)
# except for the build directory
SUBDIRS := $(filter-out build/., $(SUBDIRS))
# build the nano_core separately
SUBDIRS := $(filter-out nano_core/., $(SUBDIRS))


.PHONY: all clean $(SUBDIRS) nano_core
.DEFAULT_GOAL := all 

## Loops over all subdirectories (except the build dir) and runs make <cmd> in it
## Note that while this does build the nano_core, the nano_core's build file does not
## copy any built object files into the kernel build dir, whereas the other modules do.
$(SUBDIRS): 
## If a directory has a Makefile, then we call it to build it as an individual module. 
	@if [ -f $@/Makefile ]; then \
		$(MAKE) -C $@ $(MAKECMDGOALS);  \
	fi;


## Build the nano_core separately, which is faster if done either at the beginning or end
## because of how xargo works: https://github.com/japaric/xargo/issues/86
nano_core:
	$(MAKE) -C nano_core $(MAKECMDGOALS)


## simply runs make all in all subdirs
all: $(SUBDIRS) nano_core


## simply runs make clean in all subdirs, then removes the kernel build dir
clean: $(SUBDIRS) nano_core
	rm -rf $(KERNEL_BUILD_DIR)

