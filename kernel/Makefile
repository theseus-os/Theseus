### This Makefile builds only the nano_core object files (as a static library ".a" file), 
### consisting of the nano_core's Rust crate modules and its assembly boot files.
### It does not build an OS .iso image or anything runnable in a simulator like QEMU.

## defines all config options for building any kernel crate
## NOTE: most of the variables used below are defined in Config.mk
include Config.mk  

## this is the output path of the xargo command, not our choice.
## The name of the lib (lib_____.a) is defined by the target above.
cargo-obj := target/$(TARGET)/$(BUILD_MODE)/libnano_core.a

nano_core-src := nano_core/src
nano_core-obj := $(KERNEL_BUILD_DIR)/nano_core-$(ARCH).bin

linker_script := $(nano_core-src)/boot/arch_$(ARCH)/linker_higher_half.ld
assembly_source_files := $(wildcard $(nano_core-src)/boot/arch_$(ARCH)/*.asm)
assembly_object_files := $(patsubst $(nano_core-src)/boot/arch_$(ARCH)/%.asm, \
	$(KERNEL_BUILD_DIR)/boot/$(ARCH)/%.o, $(assembly_source_files))


.PHONY: all clean cargo doc

all: $(nano_core-obj)

clean:
	cargo clean
	rm -rf ./build


cargo: 
	RUST_TARGET_PATH="${CFG_DIR}" RUSTFLAGS="${RUSTFLAGS}" xargo build --all $(XARGO_RELEASE_ARG) $(RUST_FEATURES)  --target $(TARGET)  ## target arg must be a target name, not the file path.


$(nano_core-obj): cargo $(cargo-obj) $(assembly_object_files) $(linker_script)
	@mkdir -p $(KERNEL_BUILD_DIR)
	ld -n -T $(linker_script) -o $(nano_core-obj) $(assembly_object_files) $(cargo-obj)
	@readelf -S -s -W $(nano_core-obj) | sed '/LOCAL  /d;/WEAK   /d' > $(KERNEL_BUILD_DIR)/nano_core.sym   # removes LOCAL and WEAK symbols from the ELF file
	@for objfile in ./target/$(TARGET)/${BUILD_MODE}/deps/*.o ; do \
		cp -vf $${objfile}  $(KERNEL_BUILD_DIR)/`basename $${objfile} | sed -n -e 's/\(.*\)-.*.o/\1.o/p'` ; \
	done

# compile assembly files
$(KERNEL_BUILD_DIR)/boot/$(ARCH)/%.o: $(nano_core-src)/boot/arch_$(ARCH)/%.asm
	@mkdir -p $(shell dirname $@)
	@nasm -felf64 $< -o $@


## Builds Theseus's documentation.
## The "doc" directory is excluded from the workspace so it is not accidentally built as code, even though it contains the top-level docs.
## Instead, each file in the "doc" directory, including main lib.rs file, is temporarily moved to the top-level kernel workspace directory
## so that when compiled with rustdoc, all of the workspace's subcrates show up nicely in the sidebar.
doc:
	@cargo doc --no-deps
	@cp -f doc/src/lib.rs __doc.rs
	@rustdoc --output target/doc --crate-name Theseus __doc.rs
## TODO: FIXME: build all other .rs files in the "doc" crate... do we give them the same "Theseus" crate name?
	@rm -f __doc.rs
	@ln -f -s ./Theseus/index.html target/doc/index.html

