SHELL := /bin/bash
APPS_BUILD_DIR ?= $(shell pwd)/build
export  # export this to Makefiles in subdirectories


# get all the subdirectories for making
SUBDIRS := $(wildcard */.)
# except for the build directory and those starting with a dot
SUBDIRS := $(filter-out build/., $(SUBDIRS))
SUBDIRS := $(filter-out .*/, $(SUBDIRS))


# uncomment the line blow to manually specifying which app directories to build
#SUBDIRS := hello

.PHONY: all clean $(SUBDIRS)
.DEFAULT_GOAL := all


all: $(SUBDIRS)
$(SUBDIRS):
	$(MAKE) -C $@ $(MAKECMDGOALS)


## simply runs make clean in all subdirs, then removes the build directory
clean: $(SUBDIRS)
	@rm -rf $(APPS_BUILD_DIR)

