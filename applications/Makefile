### This makefile is run once to build each application directory.
### It is run by the Makefile in applications/ directory, 
### but actually runs within the context of eact subdirectory.

BUILD_MODE := release

## Since applications are run in the kernel, we use the same Config.mk file 
## as the one that defines all config options for building regular kernel crates.
include ../kernel/Config.mk   

## Applications are forbidden from using unsafe code
# COMPILER_LINTS += -D unsafe-code

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



.PHONY: all clean cargo build 
.DEFAULT_GOAL := all
all: build



clean:
	cargo clean
	rm -rf ./build


### TODO FIXME: change this to use xargo rustc so we can feed in the COMPILER_LINTS argument, 
### However, I cannot figure out how to do this while using a cargo virtual workspace.
### (Solution: I guess we could just invoke xargo rustc in each subdirectory manually)
cargo:
### For the "cargo rustc" command above, all of the arguments to cargo come before the "--",
### whereas all of the arguments to rustc come after the "--".
	@RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" \
		xargo build \
		$(XARGO_RELEASE_ARG) \
		--target $(TARGET) \
		-- \
		# $(COMPILER_LINTS)


### This first calls the cargo target, but then copies all application object files into the build dir. 
build: cargo
	@mkdir -p ./build
	@for app in $(APPS) ; do  \
		cp -vf `ls -t ./target/$(TARGET)/$(BUILD_MODE)/deps/$${app}*.o | head -n1`  ./build/$${app}.o ; \
		strip --strip-debug  ./build/$${app}.o ; \
	done
