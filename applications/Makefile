### This makefile is run once to build each application directory.
### It is run by the Makefile in applications/ directory, 
### but actually runs within the context of eact subdirectory.

BUILD_MODE := release

## Since applications are run in the kernel, we use the same Config.mk file 
## as the one that defines all config options for building regular kernel crates.
include ../cfg/Config.mk   


## The directory where we store custom lints (compiler plugins)
COMPILER_PLUGINS_DIR = $(ROOT_DIR)/compiler_plugins


## Applications are forbidden from using unsafe code
COMPILER_LINTS += -D unsafe-code

## Applications must have a main function
COMPILER_LINTS += --extern application_main_fn=$(COMPILER_PLUGINS_DIR)/target/$(BUILD_MODE)/libapplication_main_fn.so  \
				  -Z extra-plugins=application_main_fn \
				  -D application_main_fn


## these don't work -- put them in the Cargo.toml file instead
# RUSTFLAGS += -C codegen-units=1
# RUSTFLAGS += -C incremental=false


# get all the subdirectories, i.e., the list of applications
APPS := $(wildcard */.)
# exclude the build directory 
APPS := $(filter-out build/. target/., $(APPS))
# exclude hidden directories starting with a "."
APPS := $(filter-out .*/, $(APPS))
# remove the trailing /. on each name
APPS := $(patsubst %/., %, $(APPS))



.PHONY: all clean cargo build compiler_plugins
.DEFAULT_GOAL := all
all: build



clean:
	cargo clean
	rm -rf ./build



### Builds our custom lints in the compiler plugins directory so we can use them here
compiler_plugins:
	@cd $(COMPILER_PLUGINS_DIR) && cargo build $(XARGO_RELEASE_ARG)


cargo: compiler_plugins
### For the "xargo rustc" command below, all of the arguments to cargo/xargo come before the "--",
### whereas all of the arguments to rustc come after the "--".
# for app in $(APPS) ; do  \
# 	cd $${app} ; \
# 	RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" \
# 		xargo rustc \
# 		$(XARGO_RELEASE_ARG) \
# 		--target $(TARGET) \
# 		-- \
# 		$(COMPILER_LINTS) ; \
# 	cd .. ; \
# done
### Old command
	RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" xargo build $(XARGO_RELEASE_ARG) --all --target $(TARGET)
	

### This first calls the cargo target, but then copies all object files into the build dir. 
### It gives all object files the KERNEL_PREFIX, except for "executable" application object files that get the APP_PREFIX.
build: cargo
	@mkdir -p ./build

	@for f in ./target/$(TARGET)/$(BUILD_MODE)/deps/*.o ; do \
		cp -vf $${f}  ./build/`basename $${f} | sed -n -e 's/\(.*\)-.*/$(KERNEL_PREFIX)\1\.o/p'` 2> /dev/null ; \
	done
	
	@for app in $(APPS) ; do  \
		mv  ./build/$(KERNEL_PREFIX)$${app}.o  ./build/$(APP_PREFIX)$${app}.o ; \
		strip --strip-debug  ./build/$(APP_PREFIX)$${app}.o ; \
	done
