TARGET=aarch64-unknown-none

CARGOFLAGS += --workspace
CARGOFLAGS += --release

BUILD_STD_CARGOFLAGS += -Z unstable-options
BUILD_STD_CARGOFLAGS += -Z build-std=core,alloc
BUILD_STD_CARGOFLAGS += -Z build-std-features=compiler-builtins-mem

RUSTFLAGS += -C code-model=large
RUSTFLAGS += -C relocation-model=static

QEMU_EFI_ZIP_URL ?= https://github.com/theseus-os/prebuilt-aavmf/blob/2c7254d98ee98297ad0db243bb21ed5a631d2e50/qemu-efi.fd.zip?raw=true

AARCH64_DIR ?= .
BUILD_DIR ?= $(AARCH64_DIR)/build
NATIVE_OUTPUT ?= $(AARCH64_DIR)/target/$(TARGET)/release/nano_core
nano_core_binary ?= $(BUILD_DIR)/nano_core-aarch64.bin
iso ?= $(BUILD_DIR)/kernel.elf
efi_firmware := $(AARCH64_DIR)/resources/qemu-efi.fd

# Configure the machine
QEMU_FLAGS += -machine virt
QEMU_FLAGS += -cpu cortex-a72
QEMU_FLAGS += -smp 4
QEMU_MEMORY ?= 512M
QEMU_FLAGS += -net none
QEMU_FLAGS += -device ramfb
QEMU_FLAGS += -monitor telnet:localhost:1235,server,nowait

# Include that file which enables efi support in qemu
# QEMU_FLAGS += -drive if=pflash,format=raw,file=$(efi_firmware)
QEMU_FLAGS += -bios $(efi_firmware)

QEMU_FLAGS += -drive format=raw,file=$(iso)


.PHONY: all run cargo-build create-build-structures build $(iso) $(nano_core_binary)


all: run

create-build-structure:
	mkdir -p $(BUILD_DIR)

## This target invokes the actual Rust build process via `cargo`.
cargo-build: create-build-structure
	@echo -e "\n=================== BUILDING ALL CRATES ==================="
	@echo -e "\t TARGET: \"$(TARGET)\""
	RUSTFLAGS='$(RUSTFLAGS)' cargo build $(CARGOFLAGS) $(BUILD_STD_CARGOFLAGS) --target $(TARGET)

## This builds the nano_core binary itself, which is the fully-linked code that first runs right after the bootloader
$(nano_core_binary): cargo-build
	cp $(NATIVE_OUTPUT) $(nano_core_binary)

build: $(nano_core_binary)

$(iso): build
	cargo r \
		--release \
		-Z bindeps \
		--manifest-path \
		$(AARCH64_DIR)/../tools/uefi_builder/aarch64/Cargo.toml -- \
		--kernel $(nano_core_binary) \
		--efi-image $(iso)

## This downloads the binary blob making Qemu compatible with UEFI
$(efi_firmware):
	wget $(QEMU_EFI_ZIP_URL) -O qemu-efi.zip
	@# this will extract to ./resources/qemu-efi.fd
	unzip qemu-efi.zip
	rm qemu-efi.zip

### builds and runs Theseus in QEMU
run: $(iso) $(efi_firmware)
	qemu-system-aarch64 $(QEMU_FLAGS)
