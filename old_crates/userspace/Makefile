SHELL := /bin/bash
USERSPACE_BUILD_DIR ?= $(shell pwd)/build
export  # export this to Makefiles in subdirectories


# get all the subdirectories for making
SUBDIRS := $(wildcard */.)
# except for the build directory
SUBDIRS := $(filter-out build/., $(SUBDIRS))


.PHONY: all clean $(SUBDIRS)
.DEFAULT_GOAL := all


all: $(SUBDIRS)
$(SUBDIRS):
	$(MAKE) -C $@ $(MAKECMDGOALS)


## simply runs make clean in all subdirs, then removes the userspace build dir
clean: $(SUBDIRS)
	@rm -rf $(USERSPACE_BUILD_DIR)

