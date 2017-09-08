### This Makefile builds all of the subdirectories within this kernel directory.
### It offers the "all" and "clean" targets only

KERNEL_BUILD_DIR = build
export  # export this to Makefiles in subdirectories

TOPTARGETS := all clean

# get all the subdirectories for making
SUBDIRS := $(wildcard */.)
# except for the build directory
SUBDIRS := $(filter-out $(KERNEL_BUILD_DIR)/., $(SUBDIRS))



## TODO: change this, because most directories will jsut be built with cargo directly, 
##       they won't all have their own individual makefile

$(TOPTARGETS): $(SUBDIRS)
$(SUBDIRS):
	$(MAKE) -C $@ $(MAKECMDGOALS)

.PHONY: $(TOPTARGETS) $(SUBDIRS)
