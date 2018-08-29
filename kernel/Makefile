### This Makefile builds only the nano_core object files (as a static library ".a" file), 
### consisting of the nano_core's Rust crate modules and its assembly boot files.
### It does not build an OS .iso image or anything runnable in a simulator like QEMU.

## most of the variables used below are defined in Config.mk
include ../cfg/Config.mk  

KERNEL_BUILD_DIR := ./build

## this is the output path of the xargo command, not our choice.
## The name of the lib (lib_____.a) is defined by the target above.
cargo-obj := target/$(TARGET)/$(BUILD_MODE)/libnano_core.a

nano_core-src := nano_core/src
nano_core-obj := $(KERNEL_BUILD_DIR)/nano_core-$(ARCH).bin

linker_script := $(nano_core-src)/boot/arch_$(ARCH)/linker_higher_half.ld
assembly_source_files := $(wildcard $(nano_core-src)/boot/arch_$(ARCH)/*.asm)
assembly_object_files := $(patsubst $(nano_core-src)/boot/arch_$(ARCH)/%.asm, \
	$(KERNEL_BUILD_DIR)/boot/$(ARCH)/%.o, $(assembly_source_files))


# get all the subdirectories, i.e., the list of all kernel crates
KERNEL_CRATES := $(wildcard */.)
# exclude the build directory 
KERNEL_CRATES := $(filter-out build/. target/., $(KERNEL_CRATES))
# exclude hidden directories starting with a "."
KERNEL_CRATES := $(filter-out .*/, $(KERNEL_CRATES))
# remove the trailing /. on each name
KERNEL_CRATES := $(patsubst %/., %, $(KERNEL_CRATES))



.PHONY: all clean cargo doc

all: $(nano_core-obj)

clean:
	cargo clean
	rm -rf $(KERNEL_BUILD_DIR)
 


cargo: 
### Old command: 
	RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" xargo build  $(PACKAGE_FEATURES) $(CARGO_OPTIONS)  $(RUST_FEATURES) --all --target $(TARGET)

# ### We tried using the "xargo rustc" command here instead of "xargo build" to avoid xargo unnecessarily rebuilding core/alloc crates,
# ### But it doesn't really seem to work (it's not the cause of xargo rebuilding everything).
# 	for kd in $(KERNEL_CRATES) ; do  \
# 		cd $${kd} ; \
# 		echo -e "\n========= BUILDING KERNEL CRATE $${kd} ==========\n" ; \
# 		RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" \
# 			xargo rustc \
# 			$(CARGO_OPTIONS) \
# 			$(RUST_FEATURES) \
# 			--target $(TARGET) ; \
# 		cd .. ; \
# 	done
	


$(nano_core-obj): cargo $(cargo-obj) $(assembly_object_files) $(linker_script)
	@mkdir -p $(KERNEL_BUILD_DIR)
	ld -n -T $(linker_script) \
		-o $(nano_core-obj) \
		$(assembly_object_files) $(cargo-obj)
# run "readelf" on the nano_core binary, and then demangle it, and then output to a sym file
	cargo run --manifest-path $(ROOT_DIR)/tools/demangle_readelf_file/Cargo.toml <(readelf -S -s -W $(nano_core-obj) | sed '/LOCAL  /d;/WEAK   /d') > $(KERNEL_BUILD_DIR)/$(KERNEL_PREFIX)nano_core.sym   # removes LOCAL and WEAK symbols from the ELF file
# copy the object files into the kernel build directory
	cargo run --manifest-path $(ROOT_DIR)/tools/copy_latest_object_files/Cargo.toml --  -v --prefix $(KERNEL_PREFIX) ./target/$(TARGET)/$(BUILD_MODE)/deps/  $(KERNEL_BUILD_DIR)
	# @for objfile in ./target/$(TARGET)/$(BUILD_MODE)/deps/*.o ; do \
	# 	cp -vf $${objfile}  $(KERNEL_BUILD_DIR)/`basename $${objfile} | sed -n -e 's/\(.*\)-.*.o/$(KERNEL_PREFIX)\1.o/p'` ; \
	# done

# compile assembly files
$(KERNEL_BUILD_DIR)/boot/$(ARCH)/%.o: $(nano_core-src)/boot/arch_$(ARCH)/%.asm
	@mkdir -p $(shell dirname $@)
	@nasm -felf64 $< -o $@


## Builds Theseus's documentation.
## The entire project is built as normal using the "cargo doc" command,
## except for the special top-level documentation file, which we build separately.
doc:
	@cargo doc --no-deps
	@rustdoc --output target/doc --crate-name "___Theseus_Crates___" documentation/src/_top.rs

