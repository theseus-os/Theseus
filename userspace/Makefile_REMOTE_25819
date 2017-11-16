USERSPACE_BUILD_DIR = build
export  # export this to Makefiles in subdirectories

TOPTARGETS := all clean

# get all the subdirectories for making
SUBDIRS := $(wildcard */.)
# except for the build directory
SUBDIRS := $(filter-out $(USERSPACE_BUILD_DIR)/., $(SUBDIRS))


$(TOPTARGETS): $(SUBDIRS)
$(SUBDIRS):
	$(MAKE) -C $@ $(MAKECMDGOALS)

.PHONY: $(TOPTARGETS) $(SUBDIRS)
