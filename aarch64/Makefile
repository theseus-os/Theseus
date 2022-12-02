TARGET=aarch64-unknown-uefi

CARGOFLAGS += --workspace
CARGOFLAGS += --release

BUILD_STD_CARGOFLAGS += -Z unstable-options
BUILD_STD_CARGOFLAGS += -Z build-std=core,alloc
BUILD_STD_CARGOFLAGS += -Z build-std-features=compiler-builtins-mem

QEMU_EFI_ZIP_URL ?= https://github.com/theseus-os/prebuilt-aavmf/blob/2c7254d98ee98297ad0db243bb21ed5a631d2e50/qemu-efi.fd.zip?raw=true

AARCH64_DIR ?= .
BUILD_DIR ?= $(AARCH64_DIR)/build
QEMU_EFI_FD ?= $(AARCH64_DIR)/resources/qemu-efi.fd
NATIVE_OUTPUT ?= $(AARCH64_DIR)/target/aarch64-unknown-uefi/release/nano_core.efi
nano_core_binary ?= $(BUILD_DIR)/efi

# Configure the machine
QEMU_FLAGS += -machine virt
QEMU_FLAGS += -cpu cortex-a72
QEMU_FLAGS += -smp 4
QEMU_FLAGS += -m 1000
QEMU_FLAGS += -net none
# QEMU_FLAGS += -nographic

# Include that file which enables efi support in qemu
QEMU_FLAGS += -drive if=pflash,format=raw,file=$(QEMU_EFI_FD)

# Include the build dir as an emulated fat drive
QEMU_FLAGS += -drive file=fat:rw:$(BUILD_DIR)






all: run

create-build-structure:
	mkdir -p $(BUILD_DIR)

## This target invokes the actual Rust build process via `cargo`.
cargo-build: create-build-structure
	@echo -e "\n=================== BUILDING ALL CRATES ==================="
	@echo -e "\t TARGET: \"$(TARGET)\""
	cargo build $(CARGOFLAGS) $(BUILD_STD_CARGOFLAGS) --target $(TARGET)

## This builds the nano_core binary itself, which is the fully-linked code that first runs right after the bootloader
$(nano_core_binary): cargo-build
	cp $(NATIVE_OUTPUT) $(nano_core_binary)

build: $(nano_core_binary)

## This downloads the binary blob making Qemu compatible with UEFI
$(QEMU_EFI_FD):
	wget $(QEMU_EFI_ZIP_URL) -O qemu-efi.zip
	@# this will extract to ./resources/qemu-efi.fd
	unzip qemu-efi.zip
	rm qemu-efi.zip

### builds and runs Theseus in QEMU
run: build $(QEMU_EFI_FD)
	cat resources/qemu-instructions.txt
	qemu-system-aarch64 $(QEMU_FLAGS)
