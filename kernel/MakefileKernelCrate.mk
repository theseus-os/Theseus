### This makefile is run from each kernel crate, so it needs to use paths
### relative to a subdirectory, not the actual directory contaiing this file.
### So, to access the directory containing this file, you would use "../"

## defines all config options for building any kernel crate
## NOTE: most of the variables used below are defined in Config.mk
include ../Config.mk  

## the module name is derived from the name of the directory 
MODULE_NAME := $(strip $(shell basename ${PWD}))

.PHONY: all clean cargo

all: cargo

clean:
	cargo clean
	rm -rf ${KERNEL_BUILD_DIR}/__k_${MODULE_NAME}.o


## Builds the crate, and then copies all object files to its own module-specific subdirectory in the kernel build directory
cargo: 
	@mkdir -p $(KERNEL_BUILD_DIR)/${MODULE_NAME}
	echo "MODULE_NAME: $(MODULE_NAME), FEATURES: $(FEATURES_$(MODULE_NAME))"
	RUST_TARGET_PATH="${CFG_DIR}" RUSTFLAGS="${RUSTFLAGS}" xargo build $(XARGO_RELEASE_ARG) $(FEATURES_$(MODULE_NAME)) --target $(TARGET)
	@for objfile in ./target/$(TARGET)/${BUILD_MODE}/deps/*.o ; do \
		cp -vf $${objfile}  $(KERNEL_BUILD_DIR)/${MODULE_NAME}/`basename $${objfile} | sed -n -e 's/\(.*\)-.*.o/\1.o/p'` ; \
	done

#
