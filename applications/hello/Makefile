APP_NAME := $(shell basename `pwd`)

BUILD_MODE := release

## Since applications are run in the kernel, we use the same Config.mk file 
## as the one that defines all config options for building regular kernel crates.
include ../../kernel/Config.mk  

## override the cfg directory, since the relative path in the kernel directory
## is different than in the applications directory
CFG_DIR := $(shell pwd)/../../cfg


## these don't work -- put them in the Cargo.toml file instead
# RUSTFLAGS += -C codegen-units=1
# RUSTFLAGS += -C incremental=false



.PHONY: all clean cargo build

all: build


clean:
	cargo clean


cargo:
	RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" xargo build $(XARGO_RELEASE_ARG) --target $(TARGET)


build: cargo
	@mkdir -p ../build
	cp -vf ./target/$(TARGET)/$(BUILD_MODE)/deps/$(APP_NAME)*.o  ../build/$(APP_NAME).o
	@strip --strip-debug ../build/$(APP_NAME).o
