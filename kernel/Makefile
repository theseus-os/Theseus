### This Makefile builds the specified subdirectories within this kernel directory.
### It offers the "all" and "clean" targets only
SHELL := /bin/bash


KERNEL_BUILD_DIR ?= $(shell pwd)/build
export  # export this to Makefiles in subdirectories

# get all the subdirectories for making
SUBDIRS := $(wildcard */.)
# except for the build directory
SUBDIRS := $(filter-out build/., $(SUBDIRS))


.PHONY: build clean $(SUBDIRS)
.DEFAULT_GOAL := build 

## Loops over all subdirectories (except the build dir) and runs make <cmd> in it
## Note that while this does build the nano_core, the nano_core's build file does not
## copy any built object files into the kernel build dir, whereas the other modules do.
$(SUBDIRS): 
## If a directory has a Makefile, then we call it to build it as an individual module. 
	@if [ -f $@/Makefile ]; then \
		$(MAKE) -C $@ $(MAKECMDGOALS);  \
	fi;
## Otherwise, we're ignoring it for now. 


## simply runs make build in all subdirs
build: $(SUBDIRS) 


## simply runs make clean in all subdirs, then removes the kernel build dir
clean: $(SUBDIRS)
	rm -rf $(KERNEL_BUILD_DIR)

