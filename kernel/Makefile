### This Makefile builds the specified subdirectories within this kernel directory.
### It offers the "all" and "clean" targets only


export RUST_TARGET_PATH:="$(pwd)/../cfg"

KERNEL_BUILD_DIR ?= build
export  # export this to Makefiles in subdirectories

TOPTARGETS := build clean

# get all the subdirectories for making
SUBDIRS := $(wildcard */.)
# except for the build directory
SUBDIRS := $(filter-out $(KERNEL_BUILD_DIR)/., $(SUBDIRS))


.PHONY: build $(TOPTARGETS) $(SUBDIRS)
default: build ;
.DEFAULT_GOAL := build 

$(TOPTARGETS): $(SUBDIRS)
$(SUBDIRS): 
	@echo "\nBUILDING $@"
## If a directory has a Makefile, then we call it. Otherwise, we invoke cargo build. 
	@if [ -f $@/Makefile ]; then \
		$(MAKE) -C $@ $(MAKECMDGOALS);  \
	## For now, since everything is included as a regular library in the nano_core,    \
	## we don't need to individually build each crate yet as a standalone module.      \
	## When we need to do that, just uncomment the 'else' and 'xargo' lines.           \
	#else \
	#	cd $@; RUST_TARGET_PATH="../../cfg" xargo $(MAKECMDGOALS) --target x86_64-theseus; \
	fi;
