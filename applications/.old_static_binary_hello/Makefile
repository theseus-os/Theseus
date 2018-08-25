
PWD := $(shell pwd)
TARGET := x86_64-theseus
RUSTFLAGS := "-C prefer-dynamic -Z pre-link-args=-nostartfiles"


BUILD_MODE := release
ifeq ($(BUILD_MODE), release)
	CARGO_OPTIONS := --release
endif  ## otherwise, nothing, which means "debug" by default


## this is the output path of the xargo command, not our choice.
## The name of the lib (lib_____.a) is defined by the target above.
cargo-obj := target/$(TARGET)/$(BUILD_MODE)/hello



.PHONY: all clean build

all: build


clean:
	cargo clean


build: 
	RUST_TARGET_PATH="$(PWD)/../../cfg" RUSTFLAGS=$(RUSTFLAGS) xargo build $(CARGO_OPTIONS) --target $(TARGET)
