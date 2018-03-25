### This Makefile builds only the nano_core object files (as a static library ".a" file), 
### consisting of the nano_core's Rust crate modules and its assembly boot files.
### It does not build an OS .iso image or anything runnable in a simulator like QEMU.

## defines all config options for building any kernel crate
## NOTE: most of the variables used below are defined in Config.mk
include ../Config.mk  

## this is the output path of the xargo command, not our choice.
## The name of the lib (lib_____.a) is defined by the target above.
cargo-obj := target/$(TARGET)/$(BUILD_MODE)/libnano_core.a

nano_core-obj := build/nano_core-$(ARCH).bin

linker_script := src/boot/arch_$(ARCH)/linker_higher_half.ld
assembly_source_files := $(wildcard src/boot/arch_$(ARCH)/*.asm)
assembly_object_files := $(patsubst src/boot/arch_$(ARCH)/%.asm, \
	build/boot/$(ARCH)/%.o, $(assembly_source_files))


.PHONY: all clean cargo

all: $(nano_core-obj)

clean:
	cargo clean
	rm -rf ./build


cargo: 
#cargo clean   ## temporarily calling cargo clean to fix nano_core makefile
	RUST_TARGET_PATH="${CFG_DIR}" RUSTFLAGS="${RUSTFLAGS}" xargo build $(XARGO_RELEASE_ARG) ${RUST_FEATURES} --target $(TARGET)  ## target arg must be a target name, not the file path.


$(nano_core-obj): cargo $(cargo-obj) $(assembly_object_files) $(linker_script)
	@mkdir -p $(KERNEL_BUILD_DIR)
	# @ld -n --gc-sections $(KEEP_SYMBOLS) --print-gc-sections -Map=/tmp/nano_core.map -T $(linker_script) -o $(nano_core-obj) $(assembly_object_files) $(cargo-obj) 2> /tmp/nano_core_gc_sections.txt
	ld -n -T $(linker_script) -o $(nano_core-obj) $(assembly_object_files) $(cargo-obj)
	@readelf -S -s -W $(nano_core-obj) | sed '/LOCAL  /d;/WEAK   /d' > $(KERNEL_BUILD_DIR)/nano_core.sym   # removes LOCAL and WEAK symbols from the ELF file


# compile assembly files
build/boot/$(ARCH)/%.o: src/boot/arch_$(ARCH)/%.asm
	@mkdir -p $(shell dirname $@)
	@nasm -felf64 $< -o $@
