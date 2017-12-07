USERSPACE_BUILD_DIR ?= build
export  # export this to Makefiles in subdirectories

.PHONY: all clean $(SUBDIRS)
.DEFAULT_GOAL := all


# get all the subdirectories for making
SUBDIRS := $(wildcard */.)
# except for the build directory
SUBDIRS := $(filter-out $(USERSPACE_BUILD_DIR)/., $(SUBDIRS))


all: $(SUBDIRS)
$(SUBDIRS):
	$(MAKE) -C $@ $(MAKECMDGOALS)

clean:
	@rm -rf $(USERSPACE_BUILD_DIR)
