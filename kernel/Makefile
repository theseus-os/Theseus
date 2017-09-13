### This Makefile builds the specified subdirectories within this kernel directory.
### It offers the "all" and "clean" targets only

KERNEL_BUILD_DIR = build
export  # export this to Makefiles in subdirectories

TOPTARGETS := all clean

# get all the subdirectories for making
SUBDIRS := $(wildcard */.)
# except for the build directory
SUBDIRS := $(filter-out $(KERNEL_BUILD_DIR)/., $(SUBDIRS))


.PHONY: $(TOPTARGETS) $(SUBDIRS)


$(TOPTARGETS): $(SUBDIRS)
$(SUBDIRS):
## If a directory has a Makefile, then we call it. Otherwise, we invoke cargo build. 
	@if [ -f $@/Makefile ]; then \
		$(MAKE) -C $@ $(MAKECMDGOALS);  \
	# else \
		# cd $@; cargo build; \
	fi;
	# @cd serial_port; cargo build;
	# @cd vga_buffer; cargo build;


