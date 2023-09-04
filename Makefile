### This makefile is the top-level build script that builds all the crates in subdirectories 
### and combines them into the final OS .iso image.
### It also provides convenient targets for running and debugging Theseus and using GDB on your host computer.
SHELL := /bin/bash

## Disable parallelism for this Makefile since it breaks the build,
## as our dependencies aren't perfectly specified for each target.
## Cargo already handles build parallelism for us anyway.
.NOTPARALLEL:

## most of the variables used below are defined in Config.mk
include cfg/Config.mk

## By default, we just build the standard OS image via the `iso` target.
.DEFAULT_GOAL := iso

## Default values for various configuration options.
debug ?= none
net ?= none
merge_sections ?= yes
bootloader ?= grub

## aarch64 only supports booting via UEFI
ifeq ($(ARCH),aarch64)
	boot_spec = uefi
else
	boot_spec ?= bios
endif

## Set up configuration based on the chosen bootloader specification (boot_spec).
export override FEATURES+=--features nano_core/$(boot_spec)

ifeq ($(boot_spec), bios)
	ISO_EXTENSION := iso
else ifeq ($(boot_spec), uefi)
	## Disable the default "bios" feature of the nano_core
	export override FEATURES+=--no-default-features
	ISO_EXTENSION := efi
else
$(error Error:unsupported option "boot_spec=$(boot_spec)". Options are 'bios' or 'uefi')
endif

## test for Windows Subsystem for Linux (Linux on Windows)
IS_WSL = $(shell grep -is 'microsoft' /proc/version)


###################################################################################################
### Basic directory/file path definitions used throughout the makefile.
###################################################################################################
BUILD_DIR               := $(ROOT_DIR)/build
NANO_CORE_BUILD_DIR     := $(BUILD_DIR)/nano_core
iso                     := $(BUILD_DIR)/theseus-$(ARCH).$(ISO_EXTENSION)
ISOFILES                := $(BUILD_DIR)/isofiles
OBJECT_FILES_BUILD_DIR  := $(ISOFILES)/modules
DEBUG_SYMBOLS_DIR       := $(BUILD_DIR)/debug_symbols
TARGET_DEPS_DIR         := $(ROOT_DIR)/target/$(TARGET)/$(BUILD_MODE)/deps
DEPS_BUILD_DIR          := $(BUILD_DIR)/deps
HOST_DEPS_DIR           := $(DEPS_BUILD_DIR)/host_deps
DEPS_SYSROOT_DIR        := $(DEPS_BUILD_DIR)/sysroot
THESEUS_BUILD_TOML      := $(DEPS_BUILD_DIR)/TheseusBuild.toml
THESEUS_CARGO           := $(ROOT_DIR)/tools/theseus_cargo
THESEUS_CARGO_BIN       := $(THESEUS_CARGO)/bin/theseus_cargo
EXTRA_FILES             := $(ROOT_DIR)/extra_files
LIMINE_DIR              := $(ROOT_DIR)/limine-prebuilt


### Set up tool names/locations for cross-compiling on a Mac OS / macOS host (Darwin).
UNAME = $(shell uname -s)
ifeq ($(UNAME),Darwin)
	CROSS = $(ARCH)-elf-
	## macOS uses a different unmounting utility
	UNMOUNT = diskutil unmount
	USB_DRIVES = $(shell diskutil list external | grep -s "/dev/" | awk '{print $$1}')
else
	## Handle building for aarch64 on x86_64 Linux/WSL
	ifeq ($(ARCH),aarch64)
		CROSS = aarch64-linux-gnu-
	endif
	## Just use normal umount on Linux/WSL
	UNMOUNT = umount
	USB_DRIVES = $(shell lsblk -O | grep -i usb | awk '{print $$2}' | grep --color=never '[^0-9]$$')
endif

### Handle multiple bootloader options and ensure the corresponding tools are installed.
ifeq ($(boot_spec),uefi)
	## A bootloader isn't required with UEFI.
else ifeq ($(bootloader),grub)
	## Look for `grub-mkrescue` (Debian-like distros) or `grub2-mkrescue` (Fedora)
	ifneq (,$(shell command -v grub-mkrescue))
		GRUB_MKRESCUE = grub-mkrescue
	else ifneq (,$(shell command -v grub2-mkrescue))
		GRUB_MKRESCUE = grub2-mkrescue
	else
$(error Error: could not find 'grub-mkrescue' or 'grub2-mkrescue', please install 'grub' or 'grub2')
	endif
else ifeq ($(bootloader),limine)
	## Check if the limine directory exists. 
	ifneq (,$(wildcard $(LIMINE_DIR)/.))
		export override FEATURES += --features extract_boot_modules
	else
$(error Error: missing '$(LIMINE_DIR)' directory! Please follow the limine instructions in the README)
	endif
else
$(error Error: unsupported option "bootloader=$(bootloader)". Options are 'grub' or 'limine')
endif


###################################################################################################
### This section contains targets to actually build Theseus components and create an iso file.
###################################################################################################

## The linker script applied to each output file in $(OBJECT_FILES_BUILD_DIR).
partial_relinking_script := cfg/partial_linking_combine_sections.ld
## The default file path where cargo outputs the nano_core's static library.
nano_core_static_lib := $(ROOT_DIR)/target/$(TARGET)/$(BUILD_MODE)/libnano_core.a
## The output file path of the fully-linked nano_core kernel binary.
nano_core_binary := $(NANO_CORE_BUILD_DIR)/nano_core-$(ARCH).bin
## The linker script for linking the `nano_core_binary` with the compiled assembly files.
linker_script := $(ROOT_DIR)/kernel/nano_core/linker_higher_half-$(ARCH).ld
efi_firmware := $(BUILD_DIR)/$(OVMF_FILE)

ifeq ($(ARCH),x86_64)
## The assembly files compiled by the nano_core build script.
compiled_nano_core_asm := $(NANO_CORE_BUILD_DIR)/compiled_asm/$(boot_spec)/*.o
endif

## Specify which crates should be considered as application-level libraries. 
## These crates can be instantiated multiply (per-task, per-namespace) rather than once (system-wide);
## they will only be multiply instantiated if they have data/bss sections.
## Ideally we would do this with a script that analyzes dependencies to see if a crate is only used by application crates,
## but I haven't had time yet to develop that script. It would be fairly straightforward using a tool like `cargo deps`. 
## So, for now, we just do it manually.
## You can execute this to view dependencies to help you out:
## `cd kernel/nano_core && cargo deps --include-orphans --no-transitive-deps | dot -Tpdf > /tmp/graph.pdf && xdg-open /tmp/graph.pdf`
EXTRA_APP_CRATE_NAMES += getopts unicode_width

# get all the subdirectories in applications/, i.e., the list of application crates
APP_CRATE_NAMES := $(notdir $(wildcard applications/*))
# exclude the build directory 
APP_CRATE_NAMES := $(filter-out build/. target/., $(APP_CRATE_NAMES))
# exclude hidden directories starting with a "."
APP_CRATE_NAMES := $(filter-out .*/, $(APP_CRATE_NAMES))
# remove the trailing /. on each name
APP_CRATE_NAMES := $(patsubst %/., %, $(APP_CRATE_NAMES))
APP_CRATE_NAMES += $(EXTRA_APP_CRATE_NAMES)


### PHONY is the list of targets that *always* get rebuilt regardless of dependent files' modification timestamps.
### Most targets are PHONY because cargo itself handles whether or not to rebuild the Rust code base.
.PHONY: all full \
		check-usb \
		clean clean-doc clean-old-build \
		orun orun_pause run run_pause iso build cargo copy_kernel $(bootloader) extra_files \
		libtheseus \
		simd_personality_sse build_sse simd_personality_avx build_avx \
		gdb gdb_aarch64 \
		clippy doc docs view-doc view-docs book view-book


### If we compile for SIMD targets newer than SSE (e.g., AVX or newer),
### then we need to define a preprocessor variable 
### that will cause the AVX flag to be enabled in the boot-up assembly code. 
ifneq (,$(findstring avx,$(TARGET)))
export override CFLAGS+=-DENABLE_AVX
endif


### Convert `THESEUS_CONFIG` values into `RUSTFLAGS` by prepending "--cfg " to each one.
### Note: this change to RUSTFLAGS is exported as an external shell environment variable
###       in order to make it easy to pass to sub-make invocations.
###       However, this means we must not explicitly not use it for `cargo run` tool invocations,
###       because those should be built as normal for the host OS environment.
export override RUSTFLAGS += $(patsubst %,--cfg %, $(THESEUS_CONFIG))


### Convenience targets for building the entire Theseus workspace
### with all optional components included. 
### See `theseus_features/src/lib.rs` for more details on what this includes.
all: full
full : export override FEATURES += --features theseus_features/everything
ifeq (,$(findstring --workspace,$(FEATURES)))
full : export override FEATURES += --workspace
endif
full: iso


### Convenience target for building the ISO using the below $(iso) target
iso: $(iso)

### This target builds an .iso OS image from all of the compiled crates.
$(iso): clean-old-build build extra_files copy_kernel $(iso)-$(boot_spec)

## This target is invoked by the '$(iso)' target when boot_spec = 'bios'.
$(iso)-bios: $(bootloader)

## This target is invoked by the '$(iso)' target when boot_spec = 'uefi'.
$(iso)-uefi: $(efi_firmware)
	@cargo run \
		--release \
		-Z bindeps \
		--manifest-path $(ROOT_DIR)/tools/uefi_builder/$(ARCH)/Cargo.toml -- \
		--kernel $(nano_core_binary) \
		--modules $(OBJECT_FILES_BUILD_DIR) \
		--efi-image $(iso)

## Copy the kernel boot image into the proper ISOFILES directory.
## Should be invoked after building all Theseus kernel/application crates.
copy_kernel:
	@mkdir -p $(ISOFILES)/boot/
	@cp $(nano_core_binary) $(ISOFILES)/boot/kernel.bin


## This first invokes the make target that runs the actual compiler, and then copies all object files into the build dir.
## This also classifies crate object files into either "application" or "kernel" crates:
## -- an application crate is any executable application in the `applications/` directory, or a library crate that is ONLY used by other applications,
## -- a kernel crate is any crate in the `kernel/` directory, or any other crates that are used by kernel crates.
## Obviously, if a crate is used by both other application crates and by kernel crates, it is still a kernel crate. 
## Then, we give all kernel crate object files the KERNEL_PREFIX and all application crate object files the APP_PREFIX.
build: $(nano_core_binary)
## Here, the main Rust build has just occurred.
##
## First, if an .rlib archive contains multiple object files, we need to extract them all out of the archive
## and combine them into one object file using partial linking (`ld -r ...`), overwriting the rustc-emitted .o file.
## Note: we skip "normal" .rlib archives that have 2 members: a single .o object file and a single .rmeta file.
## Note: the below line with `cut` simply removes the `lib` prefix and the `.rlib` suffix from the file name.
	@for f in $(shell find $(TARGET_DEPS_DIR)/ -name "*.rlib"); do                                          \
		if [ `$(CROSS)ar -t $${f} | wc -l` != "2" ]; then                                                   \
			echo -e "\033[1;34mUnarchiving multi-file rlib: \033[0m $${f}"                                  \
				&& mkdir -p "$(BUILD_DIR)/extracted_rlibs/`basename $${f}`-unpacked/"                       \
				&& $(CROSS)ar -xo --output "$(BUILD_DIR)/extracted_rlibs/`basename $${f}`-unpacked/" $${f}  \
				&& $(CROSS)ld -r                                                                            \
					--output "$(TARGET_DEPS_DIR)/`basename $${f} | cut -c 4- | rev | cut -c 6- | rev`.o"    \
					$$(find $(BUILD_DIR)/extracted_rlibs/$$(basename $${f})-unpacked/ -name "*.o")        ; \
		fi  &                                                                                               \
	done; wait

## Second, copy all object files into the main build directory and prepend the kernel or app prefix appropriately. 
	@RUSTFLAGS="" cargo run --release --manifest-path $(ROOT_DIR)/tools/copy_latest_crate_objects/Cargo.toml -- \
		-i "$(TARGET_DEPS_DIR)" \
		--output-objects $(OBJECT_FILES_BUILD_DIR) \
		--output-deps $(DEPS_BUILD_DIR) \
		--output-sysroot $(DEPS_SYSROOT_DIR)/lib/rustlib/$(TARGET)/lib \
		-k ./kernel \
		-a ./applications \
		--kernel-prefix $(KERNEL_PREFIX) \
		--app-prefix $(APP_PREFIX) \
		-e "$(EXTRA_APP_CRATE_NAMES) libtheseus"

## Third, perform partial linking on each object file, which shrinks their size 
## and most importantly, accelerates their loading and linking at runtime.
## We also remove the unnecessary GCC_except_table* symbols from the symbol tables.
ifeq ($(merge_sections),yes)
	@for f in $(OBJECT_FILES_BUILD_DIR)/*.o ; do                                  \
		$(CROSS)ld -r -T $(partial_relinking_script) $${f} -o $${f}_relinked      \
			&& mv $${f}_relinked $${f}                                            \
			&& $(CROSS)strip --wildcard --strip-symbol=GCC_except_table* $${f}  & \
	done; wait
else ifeq ($(merge_sections),no)
# do nothing, leave the object files as is, with separate function/data sections
else
$(error Error: unsupported option "merge_sections=$(merge_sections)". Options are 'yes' or 'no')
endif

## Fourth, create the items needed for future out-of-tree builds that depend upon the parameters of this current build. 
## This includes the target file, host OS dependencies (proc macros, etc)., 
## and most importantly, a TOML file to describe these and other config variables.
	@rm -rf $(THESEUS_BUILD_TOML)
	@cp -f $(CFG_DIR)/$(TARGET).json  $(DEPS_BUILD_DIR)/
	@mkdir -p $(HOST_DEPS_DIR)
	@cp -rf ./target/$(BUILD_MODE)/deps/*  $(HOST_DEPS_DIR)/
	@echo -e 'target = "$(TARGET)"' >> $(THESEUS_BUILD_TOML)
	@echo -e 'sysroot = "./sysroot"' >> $(THESEUS_BUILD_TOML)
	@echo -e 'rustflags = "$(RUSTFLAGS)"' >> $(THESEUS_BUILD_TOML)
	@echo -e 'cargoflags = "$(CARGOFLAGS)"' >> $(THESEUS_BUILD_TOML)
	@echo -e 'features = "$(FEATURES)"' >> $(THESEUS_BUILD_TOML)
	@echo -e 'host_deps = "./host_deps"' >> $(THESEUS_BUILD_TOML)

## Fifth, strip debug information if requested. This reduces object file size, improving load times and reducing memory usage.
	@mkdir -p $(DEBUG_SYMBOLS_DIR)
ifeq ($(debug),full)
# don't strip any files
else ifeq ($(debug),none)
# strip all files
	@for f in $(OBJECT_FILES_BUILD_DIR)/*.o $(nano_core_binary) ; do \
		dbg_file=$(DEBUG_SYMBOLS_DIR)/`basename $${f}`.dbg           \
			&& cp $${f} $${dbg_file}                                 \
			&& $(CROSS)strip  --only-keep-debug  $${dbg_file}        \
			&& $(CROSS)strip  --strip-debug      $${f}             & \
	done; wait
else ifeq ($(debug),base)
# strip all object files but the base kernel
	@for f in $(OBJECT_FILES_BUILD_DIR)/*.o ; do                     \
		dbg_file=$(DEBUG_SYMBOLS_DIR)/`basename $${f}`.dbg           \
			&& cp $${f} $${dbg_file}                                 \
			&& $(CROSS)strip  --only-keep-debug  $${dbg_file}        \
			&& $(CROSS)strip  --strip-debug      $${f}             & \
	done; wait
else
$(error Error: unsupported option "debug=$(debug)". Options are 'full', 'none', or 'base')
endif

## Sixth, fix up CPU local sections.
	@echo -e "Parsing CPU local sections"
	@cargo run --release --manifest-path $(ROOT_DIR)/tools/elf_cls/Cargo.toml -- $(ARCH) --dir $(OBJECT_FILES_BUILD_DIR)

#############################
### end of "build" target ###
#############################



## This target invokes the actual Rust build process via `cargo`.
cargo:
	@mkdir -p $(BUILD_DIR)
	@mkdir -p $(NANO_CORE_BUILD_DIR)
	@mkdir -p $(OBJECT_FILES_BUILD_DIR)
	@mkdir -p $(DEPS_BUILD_DIR)

ifneq (,$(findstring vga_text_mode, $(THESEUS_CONFIG)))
	$(eval CFLAGS += -DVGA_TEXT_MODE)
endif

	@echo -e "\n=================== BUILDING ALL CRATES ==================="
	@echo -e "\t TARGET: \"$(TARGET)\""
	@echo -e "\t KERNEL_PREFIX: \"$(KERNEL_PREFIX)\""
	@echo -e "\t APP_PREFIX: \"$(APP_PREFIX)\""
	@echo -e "\t CFLAGS: \"$(CFLAGS)\""
	@echo -e "\t THESEUS_CONFIG (before build.rs script): \"$(THESEUS_CONFIG)\""
	THESEUS_CFLAGS='$(CFLAGS)' THESEUS_NANO_CORE_BUILD_DIR='$(NANO_CORE_BUILD_DIR)' RUST_TARGET_PATH='$(CFG_DIR)' RUSTFLAGS='$(RUSTFLAGS)' cargo build $(CARGOFLAGS) $(FEATURES) $(BUILD_STD_CARGOFLAGS) --target $(TARGET)

## We tried using the "cargo rustc" command here instead of "cargo build" to avoid cargo unnecessarily rebuilding core/alloc crates,
## But it doesn't really seem to work (it's not the cause of cargo rebuilding everything).
## For the "cargo rustc" command below, all of the arguments to cargo come before the "--",
## whereas all of the arguments to rustc come after the "--".
# 	for kd in $(KERNEL_CRATE_NAMES) ; do  \
# 		cd $${kd} ; \
# 		echo -e "\n========= BUILDING KERNEL CRATE $${kd} ==========\n" ; \
# 		RUST_TARGET_PATH='$(CFG_DIR)' RUSTFLAGS='$(RUSTFLAGS)' \
# 			cargo rustc \
# 			$(CARGOFLAGS) \
# 			$(RUST_FEATURES) \
# 			--target $(TARGET) ; \
# 		cd .. ; \
# 	done
# for app in $(APP_CRATE_NAMES) ; do  \
# 	cd $${app} ; \
# 	RUST_TARGET_PATH='$(CFG_DIR)' RUSTFLAGS='$(RUSTFLAGS)' \
# 		cargo rustc \
# 		$(CARGOFLAGS) \
# 		--target $(TARGET) \
# 		-- \
# 		$(COMPILER_LINTS) ; \
# 	cd .. ; \
# done


## This builds the nano_core binary itself, which is the fully-linked code that first runs right after the bootloader
$(nano_core_binary): cargo $(nano_core_static_lib) $(linker_script)
	$(CROSS)ld -n -T $(linker_script) -o $(nano_core_binary) $(compiled_nano_core_asm) $(nano_core_static_lib)
## Fix up CLS sections.
	cargo run --release --manifest-path $(ROOT_DIR)/tools/elf_cls/Cargo.toml -- $(ARCH) --file $(nano_core_binary)
## Dump readelf output for verification. See pull request #542 for more details:
##	@RUSTFLAGS="" cargo run --release --manifest-path $(ROOT_DIR)/tools/demangle_readelf_file/Cargo.toml \
##		<($(CROSS)readelf -s -W $(nano_core_binary) | sed '/OBJECT  LOCAL .* str\./d;/NOTYPE  LOCAL  /d;/FILE    LOCAL  /d;/SECTION LOCAL  /d;') \
## 		>  $(ROOT_DIR)/readelf_output
## run "readelf" on the nano_core binary, remove irrelevant LOCAL symbols from the ELF file, demangle it, serialize it, and then output to a serde file
	@RUSTFLAGS="" cargo run --release --manifest-path $(ROOT_DIR)/tools/serialize_nano_core/Cargo.toml \
		<(RUSTFLAGS="" cargo run --release --manifest-path $(ROOT_DIR)/tools/demangle_readelf_file/Cargo.toml \
		<($(CROSS)readelf -S -s -W $(nano_core_binary) \
		| sed '/OBJECT  LOCAL .* str\./d;/NOTYPE  LOCAL  /d;/FILE    LOCAL  /d;/SECTION LOCAL  /d;')) \
		> $(OBJECT_FILES_BUILD_DIR)/$(KERNEL_PREFIX)nano_core.serde
## `.sym`: this doesn't parse the object file at compile time, instead including the modified output of "readelf" as a boot module so it can then
## be parsed during boot. See pull request #542 for more details.
##	@RUSTFLAGS="" cargo run --release --manifest-path $(ROOT_DIR)/tools/demangle_readelf_file/Cargo.toml \
##		<($(CROSS)readelf -S -s -W $(nano_core_binary) | sed '/OBJECT  LOCAL .* str\./d;/NOTYPE  LOCAL  /d;/FILE    LOCAL  /d;/SECTION LOCAL  /d;') \
##		>  $(OBJECT_FILES_BUILD_DIR)/$(KERNEL_PREFIX)nano_core.sym
##	@echo -n -e '\0' >> $(OBJECT_FILES_BUILD_DIR)/$(KERNEL_PREFIX)nano_core.sym
## `.bin`: this doesn't parse the object file at compile time, instead including the nano_core binary as a boot module so it can then be parsed during
## boot. See pull request #542 for more details. 
##	@cp $(nano_core_binary) $(OBJECT_FILES_BUILD_DIR)/$(KERNEL_PREFIX)nano_core.bin


### This target auto-generates a new grub.cfg file and uses grub to build a bootable ISO.
### This target should be invoked when all of contents of `ISOFILES` are ready to be packaged into an ISO.
grub:
	@mkdir -p $(ISOFILES)/boot/grub
	@RUSTFLAGS="" cargo run --release --manifest-path $(ROOT_DIR)/tools/grub_cfg_generation/Cargo.toml -- $(ISOFILES)/modules/ -o $(ISOFILES)/boot/grub/grub.cfg
	@$(GRUB_MKRESCUE) -o $(iso) $(ISOFILES)  2> /dev/null


### This target uses limine to build a bootable ISO.
### This target should be invoked when all of contents of `ISOFILES` are ready to be packaged into an ISO.
limine:
	@cd $(OBJECT_FILES_BUILD_DIR)/ && ls | cpio --no-absolute-filenames -o > $(ISOFILES)/modules.cpio
	@RUSTFLAGS="" cargo run -r --manifest-path $(ROOT_DIR)/tools/limine_compress_modules/Cargo.toml -- -i $(ISOFILES)/modules.cpio -o $(ISOFILES)/modules.cpio.lz4
	@rm $(ISOFILES)/modules.cpio
	@cp cfg/limine.cfg $(LIMINE_DIR)/limine-cd.bin $(LIMINE_DIR)/limine-cd-efi.bin $(LIMINE_DIR)/limine.sys $(ISOFILES)/
	@rm -f $(iso)
	@xorriso -as mkisofs \
		-b limine-cd.bin -no-emul-boot -boot-load-size 4 \
		-boot-info-table --efi-boot limine-cd-efi.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		$(ISOFILES)/ -o $(iso)
	@$(MAKE) -C $(LIMINE_DIR)
	@$(LIMINE_DIR)/limine-deploy $(iso)


## This downloads the OVMF EFI firmware, needed by QEMU to boot an EFI app.
##
## These binary files are built by Github user retrage at:
## https://github.com/retrage/edk2-nightly.
$(efi_firmware):
	@echo -e "\033[1;34m\nDownloading prebuilt EFI firmware from GitHub...\033[0m"
	@wget -nv --show-progress https://raw.githubusercontent.com/retrage/edk2-nightly/$(OVMF_COMMIT)/bin/$(OVMF_FILE) -O $(efi_firmware)


### This target copies all extra files into the `ISOFILES` directory,
### collapsing their directory structure into a single file name with `!` as the directory delimiter.
### The contents of the EXTRA_FILES directory will be available at runtime within Theseus's root fs, too.
### See the `README.md` in the `extra_files` directory for more info.
extra_files:
	@mkdir -p $(OBJECT_FILES_BUILD_DIR)
	@for f in $(shell cd $(EXTRA_FILES) && find * -type f); do \
		ln -f  $(EXTRA_FILES)/$${f}  $(OBJECT_FILES_BUILD_DIR)/`echo -n $${f} | sed 's/\//!/g'`  & \
	done; wait


### Target for building tlibc, Theseus's libc.
### This should be run after `make iso` has completed.
### It builds a new .iso that includes tlibc, which can be run using `make orun`.
### Currently we can manually load tlibc within Theseus for testing purposes by running `ns --load path/to/tlibc_file`.
.PHONY: tlibc
TLIBC_OBJ_FILE := tlibc/target/$(TARGET)/$(BUILD_MODE)/tlibc.o
tlibc:
# $(MAKE) -C tlibc
	( cd ./tlibc; sh build.sh )

	@for f in $(TLIBC_OBJ_FILE); do \
		$(CROSS)strip  --strip-debug  $${f} ; \
		cp -vf  $${f}  $(OBJECT_FILES_BUILD_DIR)/`basename $${f} | sed -n -e 's/\(.*\)/$(APP_PREFIX)\1/p'`   2> /dev/null ; \
	done
	$(MAKE) bootloader=$(bootloader) $(bootloader)
	@echo -e "\n\033[1;32m The build of tlibc finished successfully and was packaged into the Theseus ISO.\033[0m"
	@echo -e "    --> Use 'make orun' to run it now (don't use 'make run', that will overwrite tlibc)"
	@echo -e "    --> In Theseus, run 'ns --load /namespaces/_applications/tlibc.o' to load tlibc."



### Target for building a test C language executable.
### This should be run after `make iso` and then `make tlibc` have both completed.
.PHONY: c_test
# C_TEST_TARGET := dummy_works
C_TEST_TARGET := print_test
c_test:
	$(MAKE) -C c_test $(C_TEST_TARGET)
	@for f in c_test/$(C_TEST_TARGET); do \
		$(CROSS)strip  --strip-debug  $${f} ; \
		cp -vf  $${f}  $(OBJECT_FILES_BUILD_DIR)/`basename $${f} | sed -n -e 's/\(.*\)/$(EXECUTABLE_PREFIX)\1/p'`   2> /dev/null ; \
	done
	$(MAKE) bootloader=$(bootloader) $(bootloader)
	@echo -e "\n\033[1;32m The build of $(C_TEST_TARGET) finished successfully and was packaged into the Theseus ISO.\033[0m"
	@echo -e "    --> Use 'make orun' to run it now (don't use 'make run')"
	@echo -e "    --> In Theseus, run 'loadc /namespaces/_executables/$(C_TEST_TARGET)' to load and run the C program."



### Demo/test target for building libtheseus
libtheseus: theseus_cargo $(ROOT_DIR)/libtheseus/Cargo.* $(ROOT_DIR)/libtheseus/src/*
	@( \
		cd $(ROOT_DIR)/libtheseus && \
		$(THESEUS_CARGO_BIN) --input $(DEPS_BUILD_DIR) build; \
	)


### This target builds the `theseus_cargo` tool as a dedicated binary.
theseus_cargo: $(wildcard $(THESEUS_CARGO)/Cargo.*)  $(wildcard$(THESEUS_CARGO)/src/*)
	@echo -e "\n=================== Building the theseus_cargo tool ==================="
	cargo install --locked --force --path=$(THESEUS_CARGO) --root=$(THESEUS_CARGO)



### Removes the build directory and all compiled Rust objects.
clean:
	@rm -rf $(BUILD_DIR)
	cargo clean
	

### Removes only the old files that were copied into the build directory from a previous build.
### This is necessary to avoid lingering build files that aren't relevant to a new build,
### and would thus cause incremental re-builds to not work correctly.
### All other build files are left intact.
clean-old-build:
	@rm -rf $(OBJECT_FILES_BUILD_DIR)
	@rm -rf $(DEPS_BUILD_DIR)
	@rm -rf $(DEBUG_SYMBOLS_DIR)


# ## (This is currently not used in Theseus, since we don't run anything in userspace)
# ## This builds all userspace programs
# userspace: 
# 	@echo -e "\n======== BUILDING USERSPACE ========"
# 	@$(MAKE) -C old_crates/userspace all
# ## copy userspace binary files and add the __u_ prefix
# 	@mkdir -p $(ISOFILES)/modules
# 	@for f in `find $(ROOT_DIR)/old_crates/userspace/build -type f` ; do \
# 		cp -vf $${f}  $(ISOFILES)/modules/`basename $${f} | sed -n -e 's/\(.*\)/__u_\1/p'` 2> /dev/null ; \
# 	done



## TODO FIXME: fix up the applications build procedure so we can use lints for them, such as disabling unsafe code.
# ## The directory where we store custom lints (compiler plugins)
# COMPILER_PLUGINS_DIR = $(ROOT_DIR)/compiler_plugins
# ## Applications are forbidden from using unsafe code
# COMPILER_LINTS += -D unsafe-code
# ## Applications must have a main function
# COMPILER_LINTS += --extern application_main_fn=$(COMPILER_PLUGINS_DIR)/target/$(BUILD_MODE)/libapplication_main_fn.so  \
# 				  -Z extra-plugins=application_main_fn \
# 				  -D application_main_fn
#
# ## Builds our custom lints in the compiler plugins directory so we can use them here
# compiler_plugins:
# 	@cd $(COMPILER_PLUGINS_DIR) && cargo build $(CARGOFLAGS)




## This is a special target that enables SIMD personalities.
## It builds everything with the SIMD-enabled x86_64-unknown-theseus-sse target,
## and then builds everything again with the regular x86_64-unknown-theseus target. 
## The "normal" target must come last ('build_sse', THEN the regular 'build') to ensure that the final nano_core_binary is non-SIMD.
simd_personality_sse : export TARGET := x86_64-unknown-theseus
simd_personality_sse : export BUILD_MODE = release
simd_personality_sse : export override THESEUS_CONFIG += simd_personality simd_personality_sse
simd_personality_sse: clean-old-build build_sse build
## after building all the modules, copy the kernel boot image files
	@echo -e "********* AT THE END OF SIMD_BUILD: TARGET = $(TARGET), KERNEL_PREFIX = $(KERNEL_PREFIX), APP_PREFIX = $(APP_PREFIX)"
	$(MAKE) bootloader=$(bootloader) copy_kernel $(bootloader)
## run it in QEMU
	$(QEMU_BIN) $(QEMU_FLAGS)



## This target is like "simd_personality_sse", but uses AVX instead of SSE.
## It builds everything with the SIMD-enabled x86_64-unknown-theseus-avx target,
## and then builds everything again with the regular x86_64-unknown-theseus target. 
## The "normal" target must come last ('build_avx', THEN the regular 'build') to ensure that the final nano_core_binary is non-SIMD.
simd_personality_avx : export TARGET := x86_64-unknown-theseus
simd_personality_avx : export BUILD_MODE = release
simd_personality_avx : export override THESEUS_CONFIG += simd_personality simd_personality_avx
simd_personality_avx : export override CFLAGS += -DENABLE_AVX
simd_personality_avx: clean-old-build build_avx build
## after building all the modules, copy the kernel boot image files
	@echo -e "********* AT THE END OF SIMD_BUILD: TARGET = $(TARGET), KERNEL_PREFIX = $(KERNEL_PREFIX), APP_PREFIX = $(APP_PREFIX)"
	$(MAKE) bootloader=$(bootloader) copy_kernel $(bootloader)
## run it in QEMU
	$(QEMU_BIN) $(QEMU_FLAGS)



### build_sse builds the kernel and applications with the x86_64-unknown-theseus-sse target.
### It can serve as part of the simd_personality_sse target.
build_sse : export override TARGET := x86_64-unknown-theseus-sse
build_sse : export override RUSTFLAGS += -C no-vectorize-loops
build_sse : export override RUSTFLAGS += -C no-vectorize-slp
build_sse : export KERNEL_PREFIX := ksse\#
build_sse : export APP_PREFIX := asse\#
build_sse:
	$(MAKE) build


### build_avx builds the kernel and applications with the x86_64-unknown-theseus-avx target.
### It can serve as part of the simd_personality_avx target.
build_avx : export override TARGET := x86_64-unknown-theseus-avx
build_avx : export override RUSTFLAGS += -C no-vectorize-loops
build_avx : export override RUSTFLAGS += -C no-vectorize-slp
build_avx : export KERNEL_PREFIX := kavx\#
build_avx : export APP_PREFIX := aavx\#
build_avx:
	$(MAKE) build


### build_server is a target that builds Theseus into a regular ISO
### and then sets up an HTTP server that provides module object files 
### for a running instance of Theseus to download for OTA live updates.
build_server: preserve_old_modules iso
	OLD_MODULES_DIR=$(OBJECT_FILES_BUILD_DIR)_old \
		NEW_MODULES_DIR=$(OBJECT_FILES_BUILD_DIR) \
		NEW_DIR_NAME=$(UPDATE_DIR) \
		bash scripts/build_server.sh

preserve_old_modules:
	@mv $(OBJECT_FILES_BUILD_DIR) $(OBJECT_FILES_BUILD_DIR)_old
	cargo clean


###################################################################################################
########################### Targets for clippy and documentation ##################################
###################################################################################################

## Runs clippy on a full build of Theseus, with all crates included.
## Note that this does not cover all combinations of features or cfg values.
##
## We allow building with THESEUS_CONFIG options, but not with any other RUSTFLAGS,
## because the default RUSTFLAGS used to build Theseus aren't compatible with clippy.
ifeq ($(ARCH),x86_64)
clippy : export override FEATURES += --features theseus_features/everything
else ifeq ($(ARCH),aarch64)
clippy : export override FEATURES := $(subst --workspace,,$(FEATURES))
endif
clippy : export override RUSTFLAGS = $(patsubst %,--cfg %, $(THESEUS_CONFIG))
clippy:
	RUST_TARGET_PATH='$(CFG_DIR)' RUSTFLAGS='$(RUSTFLAGS)' \
		cargo clippy \
		$(BUILD_STD_CARGOFLAGS) $(FEATURES) \
		--target $(TARGET) \
		-- -D clippy::all


## The output directory for source-level documentation.
RUSTDOC_OUT      := $(BUILD_DIR)/doc
RUSTDOC_OUT_FILE := $(RUSTDOC_OUT)/___Theseus_Crates___/index.html

## Builds Theseus's source-level documentation for all Rust crates except applications.
## The entire project is built as normal using the `cargo doc` command (`rustdoc` under the hood).
docs: doc
doc : export override RUSTDOCFLAGS += -A rustdoc::private_intra_doc_links
doc : export override RUSTFLAGS=
doc : export override CARGOFLAGS=
doc:
## Build the docs for select library crates, namely those not hosted online.
## We do this first such that the main `cargo doc` invocation below can see and link to them.
	@cargo doc --no-deps \
		--package atomic_linked_list \
		--package cow_arc \
		--package debugit \
		--package dereffer \
		--package dfqueue \
		--package irq_safety \
		--package keycodes_ascii \
		--package lockable \
		--package locked_idt \
		--package mouse_data \
		--package owned_borrowed_trait \
		--package percent-encoding \
		--package port_io \
		--package range_inclusive \
		--package stdio \
		--package str_ref
## Now, build the docs for all of Theseus's main kernel crates.
	@cargo doc --workspace --no-deps $(addprefix --exclude , $(APP_CRATE_NAMES)) --features nano_core/bios
	@rustdoc --output target/doc --crate-name "___Theseus_Crates___" $(ROOT_DIR)/kernel/_doc_root.rs
	@rm -rf $(RUSTDOC_OUT)
	@mkdir -p $(RUSTDOC_OUT)
	@cp -rf target/doc/. $(RUSTDOC_OUT)
	@echo -e "\nTheseus source docs are now available at: \"$(RUSTDOC_OUT_FILE)\"."


## Opens the documentation root in the system's default browser. 
## the "powershell" command is used on Windows Subsystem for Linux
view-docs: view-doc
view-doc: doc
	@echo -e "Opening documentation index file in your browser..."
ifneq ($(IS_WSL), )
	wslview "$(shell realpath --relative-to="$(ROOT_DIR)" "$(RUSTDOC_OUT_FILE)")" &
else
	@xdg-open $(RUSTDOC_OUT_FILE) > /dev/null 2>&1 || open $(RUSTDOC_OUT_FILE) &
endif


### The locations of Theseus's book-style documentation.
BOOK_SRC      := $(ROOT_DIR)/book
BOOK_OUT      := $(BUILD_DIR)/book
BOOK_OUT_FILE := $(BOOK_OUT)/html/index.html

### Builds the Theseus book-style documentation using `mdbook`.
book: $(wildcard $(BOOK_SRC)/src/*) $(BOOK_SRC)/book.toml
ifneq ($(shell mdbook --version > /dev/null 2>&1 && echo $$?), 0)
	@echo -e "\nError: please install mdbook:"
	@echo -e "    cargo +stable install mdbook --force"
	@echo -e "You can optionally install linkcheck too:"
	@echo -e "    cargo +stable install mdbook-linkcheck --force"
	@exit 1
endif
	@mdbook build $(MDBOOK_ARGS) $(BOOK_SRC) -d $(BOOK_OUT)
	@echo -e "\nThe Theseus Book is now available at \"$(BOOK_OUT_FILE)\"."


### Opens the Theseus book.
export override MDBOOK_ARGS+=--open
view-book: book
	@echo -e "Opened the Theseus book in your browser."


### Removes all built documentation
clean-doc:
	@cargo clean --doc
	@rm -rf $(RUSTDOC_OUT) $(BOOK_OUT)
	

### The primary documentation for this makefile itself.
help: 
	@echo -e "\nThe following make targets are available:"
	@echo -e "   iso:"
	@echo -e "\t The default and most basic target. Builds Theseus OS with the default feature set and creates a bootable ISO image."

	@echo -e "   all:"
	@echo -e "   full:"
	@echo -e "\t Same as 'iso', but builds all Theseus OS crates by enabling the 'theseus_features/everything' feature."

	@echo -e "   run:"
	@echo -e "\t Builds Theseus (via the 'iso' target) and runs it using QEMU."

	@echo -e "   run_pause:"
	@echo -e "\t Same as 'run', but pauses QEMU at its GDB stub entry point,"
	@echo -e "\t which waits for you to connect a GDB debugger using 'make gdb'."

	@echo -e "   orun:"
	@echo -e "\t Runs the existing build of Theseus using QEMU, without building Theseus first."

	@echo -e "   orun_pause:"
	@echo -e "\t Same as 'orun', but pauses QEMU at its GDB stub entry point,"
	@echo -e "\t which waits for you to connect a GDB debugger using 'make gdb'."

	@echo -e "   loadable:"
	@echo -e "\t Same as 'run', but enables the 'loadable' configuration so that all crates are dynamically loaded."

	@echo -e "   wasmtime:"
	@echo -e "\t Same as 'run', but includes the 'wasmtime' crates in the build."

	@echo -e "   gdb:"
	@echo -e "\t Runs a new instance of GDB that connects to an already-running x86_64 QEMU instance."
	@echo -e "\t You must run an instance of Theseus on x86_64 in QEMU beforehand in a separate terminal."

	@echo -e "   gdb_aarch64:"
	@echo -e "\t Runs a new instance of GDB multiarch that connects to an already-running aarch64 QEMU instance."
	@echo -e "\t You must run an instance of Theseus on aarch64 in QEMU beforehand in a separate terminal."

	@echo -e "   bochs:"
	@echo -e "\t Same as 'make run', but runs Theseus in the Bochs emulator instead of QEMU."

	@echo -e "   usb:"
	@echo -e "\t Builds Theseus as a bootable .iso and writes it to the specified USB drive."
	@echo -e "\t The USB drive is specified as drive=<dev-name>, e.g., 'make usb drive=sdc',"
	@echo -e "\t in which the USB drive is connected as /dev/sdc. This target requires sudo."

	@echo -e "   pxe:"
	@echo -e "\t Builds Theseus as a bootable .iso and copies it to the tftpboot folder for network booting over PXE."
	@echo -e "\t You can specify a new network device with netdev=<interface-name>, e.g., 'make pxe netdev=eth0'."
	@echo -e "\t You can also specify the IP address with 'ip=<addr>'. This target requires sudo."

	@echo -e "   simd_personality_[sse|avx]:"
	@echo -e "\t Builds Theseus with a regular personality and a SIMD-enabled personality (either SSE or AVX),"
	@echo -e "\t then runs it just like the 'make run' target."

	@echo -e "   build_server:"
	@echo -e "\t Builds Theseus (as with the 'iso' target) and then runs a build server hosted on this machine"
	@echo -e "\t that can be used for over-the-air live evolution."
	@echo -e "\t You can specify the name of the directory of newly-built modules by setting the 'UPDATE_DIR' environment variable."
	@echo -e "\t This target should be invoked as an incremental build after a prior build has already completed."
	@echo -e "\t For example, first checkout version 1 (e.g., a specific git commit), build it as normal,"
	@echo -e "\t then checkout version 2 (or otherwise make some changes) and run 'make build_server'."
	@echo -e "\t Then, a running instance of Theseus version 1 can contact this machine's build_server to update itself to version 2."
	
	@echo -e "\nThe following key-value options are available to select a bootloader:"
	@echo -e "   bootloader=grub|limine"
	@echo -e "\t Configure which bootloader to pack into the final \".iso\" file."
	@echo -e "\t    'grub':    Use the GRUB bootloader. Default value."
	@echo -e "\t    'limine':  Use the Limine bootloader. See setup instructions in the README."

	@echo -e "\nThe following key-value options are available to customize the build process:"
	@echo -e "   merge_sections=yes|no"
	@echo -e "\t Choose whether sections in crate object files are merged together."
	@echo -e "\t This *significantly* improves crate load times and reduces memory usage,"
	@echo -e "\t though it may present problems for crate swapping for evolution and fault recovery."
	@echo -e "\t This is strictly a post-compilation action, it doesn't affect how code is compiled."
	@echo -e "   debug=full|base|none"
	@echo -e "\t Configure which debug symbols are stripped from the build artifacts."
	@echo -e "\t Stripped symbols are placed into files ending with \".dbg\" in \"$(DEBUG_SYMBOLS_DIR)\"."
	@echo -e "\t This is strictly a post-compilation action, it doesn't affect how code is compiled."
	@echo -e "\t    'full':   Keep debug symbols in all files, including the base kernel image and all crate object files."
	@echo -e "\t    'base':   Keep debug symbols in only the base kernel image; strip debug symbols from crate object files."
	@echo -e "\t    'none':   Strip debug symbols from both the base kernel image and all crate object files."
	@echo -e "\t              This is the default option, because it is the fastest to boot."

	@echo -e "\nThe following key-value options are available for QEMU targets, like 'run':"
	@echo -e "   net=user|tap|none"
	@echo -e "\t Configure networking in the QEMU guest:"
	@echo -e "\t    'user':  Enable networking with an e1000 NIC in the guest and a userspace SLIRP-based interface in the host (QEMU default)."
	@echo -e "\t    'tap' :  Enable networking with an e1000 NIC in the guest and a TAP interface in the host."
	@echo -e "\t    'none':  Disable all networking in the QEMU guest. This is the default behavior if no other 'net' option is provided."
# @echo -e "   kvm=yes:"
# @echo -e "\t Enable KVM acceleration (the host computer must support it)."
	@echo -e "   host=yes"
	@echo -e "\t Enable KVM and use the host CPU model. This is required for using certain x86 hardware not supported by QEMU, e.g., PMU, AVX."
	@echo -e "   int=yes"
	@echo -e "\t Enable interrupt logging in QEMU console (-d int). This is VERY verbose and slow."
	@echo -e "   vfio=<PCI_DEVICE_SLOT>"
	@echo -e "\t Use VFIO-based PCI device assignment (passthrough) in QEMU for the given device slot, e.g 'vfio=59:00.0'"
	@echo -e "   SERIAL<N>=<BACKEND>"
	@echo -e "\t Connect a guest OS serial port (e.g., 'SERIAL1' or 'SERIAL2') to a QEMU-supported backend."
	@echo -e "\t For example, 'SERIAL2=pty' will connect the second serial port for the given architecture"
	@echo -e "\t ('COM2 on x86) to a newly-allocated pseudo-terminal on Linux, e.g., '/dev/pts/6'."
	@echo -e "\t For the 'pty' option, QEMU will print a statement like so:"
	@echo -e "\t     char device redirected to /dev/pts/6 (label serial1)"
	@echo -e "\t Note that QEMU uses 0-based indexing for serial ports, so its 'serial1' label refers to the second serial port, our 'SERIAL2'."
	@echo -e "\t You can then connect to this using something like 'screen /dev/pts/6' or 'picocom /dev/pts/6',"
	@echo -e "\t or use the below 'terminal=' option to auto-launch a new terminal."
	@echo -e "\t Other options include 'stdio' (the default for 'SERIAL1'), 'file', 'pipe', etc."
	@echo -e "\t For more details, search the QEMU manual for '-serial dev'."
	@echo -e "   terminal=\"TERMINAL_COMMAND\""
	@echo -e "\t Auto-launch a new terminal window connected to the specified SERIAL<N> TTY/PTY backend"
	@echo -e "\t that QEMU created for us, as described above."
	@echo -e "\t The TERMINAL_COMMAND is specific to your system's terminal emulator and available binaries,"
	@echo -e "\t and will be invoked by our makefile with one argument: the PTY device file created by QEMU."
	@echo -e "\t For example, on a default GNOME-based Linux distro with 'screen' installed, you can run:"
	@echo -e "\t     make run terminal=\"gnome-terminal -- screen\""
	@echo -e "\t On a system with 'alacritty' installed, you can just run:"
	@echo -e "\t     make run terminal=alacritty"
	@echo -e "\t The specific syntax of TERMINAL_COMMAND depends on your host system and chosen terminal emulator."
	@echo -e "   graphic=no"
	@echo -e "\t Disable the graphical QEMU window, which reroutes the VGA text mode output to stdio."
	@echo -e "\t -- Note: the VGA device will still exist and be used by Theseus, it just will not be displayed."


	@echo -e "\nThe following make targets exist for building documentation:"
	@echo -e "   doc:"
	@echo -e "\t Builds Theseus documentation from its Rust source code (rustdoc)."
	@echo -e "   view-doc:"
	@echo -e "\t Builds Theseus documentation and then opens it in your default browser."
	@echo -e "   book:"
	@echo -e "\t Builds the Theseus book using the mdbook Markdown tool."
	@echo -e "   view-book:"
	@echo -e "\t Builds the Theseus book and then opens it in your default browser."
	@echo -e "\t If the book doesn't open in your browser, install the latest version of mdbook."
	@echo -e "   clean-doc:"
	@echo -e "\t Remove all generated documentation files."
	@echo ""




###################################################################################################
##################### This section has QEMU arguments and configuration ###########################
###################################################################################################

QEMU_FLAGS ?= 
QEMU_EXTRA ?= 
SERIAL1 ?= stdio
SERIAL2 ?= pty

ifdef IOMMU
## Currently only the `q35` machine model supports a virtual IOMMU: <https://wiki.qemu.org/Features/VT-d>
	QEMU_FLAGS += -machine q35,kernel-irqchip=split
	QEMU_FLAGS += -device intel-iommu,intremap=on,caching-mode=on
endif

## Boot from the cd-rom drive
ifeq ($(boot_spec), bios)
	QEMU_FLAGS += -cdrom $(iso) -boot d
else ifeq ($(boot_spec), uefi)
	## We use `-bios` instead of `-pflash` because `-pflash` requires the file to be exactly 64MB.
	## See:
	## - https://wiki.qemu.org/Features/PC_System_Flash
	## - https://github.com/tianocore/edk2/blob/316e6df/OvmfPkg/README#L68
	QEMU_FLAGS += -bios $(efi_firmware)
	QEMU_FLAGS += -drive format=raw,file=$(iso)
endif
## Don't reboot or shutdown upon failure or a triple reset
QEMU_FLAGS += -no-reboot -no-shutdown
## Enable a GDB stub so we can connect GDB to the QEMU instance 
QEMU_FLAGS += -s

## Enable the first serial port (the default log) to be redirected to the host terminal's stdio.
## Optionally, use the below `mon:` prefix to have the host terminal forward escape/control sequences to this serial port.
# QEMU_FLAGS += -serial $(SERIAL1)
QEMU_FLAGS += -serial mon:$(SERIAL1)

## Attach a second serial port to QEMU, which can be used for a separate headless shell/terminal.
## For example, if this is `pty`, and QEMU chooses to allocate a new pseudo-terminal at /dev/pts/6,
## then you can connect to this serial port by running a tty connector application in a new window:
## -- `screen /dev/pts/6`
## -- `picocom /dev/pts/6`
QEMU_FLAGS += -serial mon:$(SERIAL2)

## Disable the graphical display (for running in "headless" mode)
## `-vga none`:      removes the VGA card
## `-display none`:  disables QEMU's graphical display
## `-nographic`:     disables QEMU's graphical display and redirects VGA text mode output to serial.
ifeq ($(graphic), no)
	QEMU_FLAGS += -nographic
endif

## Set the amount of system memory (RAM) provided to the QEMU guest OS
QEMU_MEMORY ?= 512M
QEMU_FLAGS += -m $(QEMU_MEMORY) 

## Enable multicore CPUs, i.e., SMP (Symmetric Multi-Processor)
QEMU_CPUS ?= 4
QEMU_FLAGS += -smp $(QEMU_CPUS)

## Add a disk drive, a PATA drive over an IDE controller interface.
## Currently this is only supported on x86_64.
DISK_IMAGE ?= fat32.img
ifeq ($(ARCH),x86_64)
ifneq ($(wildcard $(DISK_IMAGE)),) 
	QEMU_FLAGS += -drive format=raw,file=fat32.img,if=ide
endif
endif

## We don't yet support SATA in Theseus, but this is how to add a SATA drive over the AHCI interface.
# QEMU_FLAGS += -drive id=my_disk,file=$(DISK_IMAGE),if=none  -device ahci,id=ahci  -device ide-drive,drive=my_disk,bus=ahci.0

## QEMU's OUI dictates that the MAC addr start with "52:54:00:"
MAC_ADDR ?= 52:54:00:d1:55:01

## Read about QEMU networking options here: https://www.qemu.org/2018/05/31/nic-parameter/
ifeq ($(net),user)
	## user-based networking setup with standard e1000 ethernet NIC
	QEMU_FLAGS += -device e1000,netdev=network0,mac=$(MAC_ADDR) -netdev user,id=network0
	## Dump network activity to a pcap file
	QEMU_FLAGS += -object filter-dump,id=f1,netdev=network0,file=netdump.pcap
else ifeq ($(net),tap)
	## TAP-based networking setup with a standard e1000 ethernet NIC frontent (in the guest) and the TAP backend (in the host)
	QEMU_FLAGS += -device e1000,netdev=network0,mac=$(MAC_ADDR) -netdev tap,id=network0,ifname=tap0,script=no,downscript=no
	## Dump network activity to a pcap file
	QEMU_FLAGS += -object filter-dump,id=f1,netdev=network0,file=netdump.pcap
else ifeq ($(net),none)
	QEMU_FLAGS += -net none
else ifneq (,$(net)) 
$(error Error: unsupported option "net=$(net)")
endif

## Dump interrupts to the serial port log
ifeq ($(int),yes)
	QEMU_FLAGS += -d int
endif

ifeq ($(host),yes)
	## KVM acceleration is required when using the host cpu model
	QEMU_FLAGS += -cpu host -accel kvm
else ifeq ($(ARCH),aarch64)
	QEMU_FLAGS += -machine virt,gic-version=3
	QEMU_FLAGS += -device ramfb
	QEMU_FLAGS += -cpu cortex-a72
	QEMU_FLAGS += -usb
	QEMU_FLAGS += -device usb-ehci,id=ehci
	QEMU_FLAGS += -device usb-kbd
else
	QEMU_FLAGS += -cpu Broadwell
endif

## Currently, kvm by itself can cause problems, but it works with the "host" option (above).
ifeq ($(kvm),yes)
$(error Error: the 'kvm=yes' option is currently broken. Use 'host=yes' instead")
	# QEMU_FLAGS += -accel kvm
endif

## Enable passthrough of a PCI device in QEMU by passing its slot information to VFIO.
## Slot information is its bus, device, and function number assigned by the host OS, e.g., 'vfio=59:00.0'.
ifdef vfio
	QEMU_FLAGS += -device vfio-pci,host=$(vfio)
endif

QEMU_FLAGS += $(QEMU_EXTRA)



###################################################################################################
########################## Targets for running and debugging Theseus ##############################
###################################################################################################

### `qemu`/`orun` (old run): runs the most recent build without rebuilding.
### This is the base-level target responsible for actually invoking QEMU;
### all other targets that want to invoke QEMU should depend on this target to do so.
qemu: orun
orun:
ifdef terminal
## Check which PTY/TTY the OS would give to the next process that requests one,
## which is the best way to guess which PTY QEMU will use for redirected serial I/O.
	$(eval temp_tty += $(shell cargo run --release --quiet --manifest-path $(ROOT_DIR)/tools/get_tty/Cargo.toml))
## If another process obtains that TTY here before we can run QEMU,
## the 'terminal' command below will connect to the wrong TTY, but there's nothing we can do.
##
## Sleep for 2 seconds to allow QEMU enough time to start and request the TTY.
	(sleep 2 && $(terminal) $(temp_tty)) & $(QEMU_BIN) $(QEMU_FLAGS)
else
	$(QEMU_BIN) $(QEMU_FLAGS)
endif


### Old Run Pause: runs the most recent build without rebuilding, but pauses QEMU until GDB is connected.
orun_pause : export override QEMU_FLAGS += -S
orun_pause: orun


### builds and runs Theseus in loadable mode, where all crates are dynamically loaded.
loadable : export override THESEUS_CONFIG += loadable
loadable: run


### builds and runs Theseus with wasmtime enabled.
wasmtime : export override FEATURES += --features wasmtime
wasmtime: run


### builds and runs Theseus in QEMU
run: $(iso) orun


### builds and runs Theseus in QEMU, but pauses execution until a GDB instance is connected.
run_pause: $(iso) orun_pause


### Runs a gdb instance on the host machine. 
### Run this after invoking another QEMU target in a different terminal.
gdb:
	@rust-os-gdb/bin/rust-gdb "$(nano_core_binary)" \
		-ex "symbol-file $(DEBUG_SYMBOLS_DIR)/`basename $(nano_core_binary)`.dbg" \
		-ex "target remote :1234"

gdb_aarch64 : override nano_core_binary=$(NANO_CORE_BUILD_DIR)/nano_core-aarch64.bin
gdb_aarch64:
	@gdb-multiarch "$(nano_core_binary)" \
		-ex "symbol-file $(DEBUG_SYMBOLS_DIR)/`basename $(nano_core_binary)`.dbg" \
		-ex "target remote :1234"


### builds and runs Theseus in Bochs
bochs : export override THESEUS_CONFIG += apic_timer_fixed
bochs: $(iso) 
	bochs -f bochsrc.txt -q




### Checks that the supplied usb device (for usage with the boot/pxe targets).
### Note: this is bypassed on WSL, because WSL doesn't support raw device files yet.
check-usb:
## on WSL, we bypass the check for USB, because burning the ISO to USB must be done with a Windows app.
ifeq ($(IS_WSL), ) ## if we're not on WSL...
## now we need to check that the user has specified a USB drive that actually exists, not a partition of a USB drive.
ifeq (,$(findstring $(drive),$(USB_DRIVES)))
	@echo -e "\nError: please specify a USB drive that exists, e.g., \"sdc\" (not a partition like \"sdc1\")."
	@echo -e "For example, run the following command:"
	@echo -e "   make boot drive=sdc\n"
	@echo -e "The following USB drives are currently attached to this system:\n$(USB_DRIVES)"
	@echo ""
	@exit 1
endif  ## end of checking that the 'drive' variable is a USB drive that exists
endif  ## end of checking for WSL


### Creates a bootable USB drive that can be inserted into a real PC based on the compiled .iso. 
usb : export override THESEUS_CONFIG += mirror_log_to_vga
usb: check-usb $(iso)
ifneq ($(IS_WSL), )
## building on WSL
	@echo -e "\n\033[1;32mThe build finished successfully\033[0m, but WSL is unable to access raw USB devices. Instead, you must burn the ISO to a USB drive yourself."
	@echo -e "The ISO file is available at \"$(iso)\"."
else
## building on Linux or macOS
	@echo -e "\n\033[1;32mThe build finished successfully.\033[0m Writing Theseus OS ISO to /dev/$(drive)..."
	@$(UNMOUNT) /dev/$(drive)* 2> /dev/null  |  true  ## force it to return true
	@sudo dd bs=4194304 if=$(iso) of=/dev/$(drive)    ## use 4194304 instead of 4M because macOS doesn't support 4M
	@sync
endif
	

### this builds an ISO and copies it into the theseus tftpboot folder as described in the REAEDME 
pxe : export override THESEUS_CONFIG += mirror_log_to_vga
pxe: $(iso)
ifdef $(netdev)
ifdef $(ip)
	@sudo ifconfig $(netdev) $(ip)
endif
	@sudo sudo ifconfig $(netdev) 192.168.1.105
endif
	@sudo cp -vf $(iso) /var/lib/tftpboot/theseus/
	@sudo systemctl restart isc-dhcp-server 
	@sudo systemctl restart tftpd-hpa
