### This makefile is the top-level build script that builds all the crates in subdirectories 
### and combines them into the final OS .iso image.
### It also provides convenient targets for running and debugging Theseus and using GDB on your host computer.
.DEFAULT_GOAL := all
SHELL := /bin/bash

## most of the variables used below are defined in Config.mk
include cfg/Config.mk

all: iso

## test for Windows Subsystem for Linux (Linux on Windows)
IS_WSL = $(shell grep -s 'Microsoft' /proc/version)


## Tool names/locations for cross-compiling on a Mac OS / macOS host (Darwin).
UNAME = $(shell uname -s)
ifeq ($(UNAME),Darwin)
	CROSS = x86_64-elf-
	## The GRUB_CROSS variable must match the build output of "scripts/mac_os_build_setup.sh"
	GRUB_CROSS = "$(HOME)"/theseus_tools_opt/bin/
	## macOS uses a different unmounting utility
	UNMOUNT = diskutil unmount
	USB_DRIVES = $(shell diskutil list external | grep -s "/dev/" | awk '{print $$1}')
else
	## Just use normal umount on Linux/WSL
	UNMOUNT = umount
	USB_DRIVES = $(shell lsblk -O | grep -i usb | awk '{print $$2}' | grep --color=never '[^0-9]$$')
endif
GRUB_MKRESCUE = $(GRUB_CROSS)grub-mkrescue



###################################################################################################
### For ensuring that the host computer has the proper version of the Rust compiler
###################################################################################################

check_rustc:
ifdef RUSTUP_TOOLCHAIN
	@echo -e 'Warning: You are overriding the Rust toolchain manually via RUSTUP_TOOLCHAIN.'
	@echo -e 'This may lead to unwanted warnings and errors during compilation.\n'
endif
	@rustup component add rust-src || (\
	echo -e "\nError: rustup is not installed on this system.";\
	echo -e "Please install rustup and try again.\n";\
	exit 1)



###################################################################################################
### For ensuring that the host computer has the proper version of xargo
###################################################################################################

XARGO_CURRENT_SUPPORTED_VERSION := 0.3.13
XARGO_OUTPUT=$(shell xargo --version 2>&1 | head -n 1)

check_xargo: 	
ifneq (${BYPASS_XARGO_CHECK}, yes)
ifneq (xargo ${XARGO_CURRENT_SUPPORTED_VERSION}, ${XARGO_OUTPUT})
	@echo -e "\nError: your xargo version does not match our supported xargo version."
	@echo -e "To install the proper version of xargo, run the following commands:\n"
	@echo -e "   cargo uninstall xargo"
	@echo -e "   cargo install --vers $(XARGO_CURRENT_SUPPORTED_VERSION) xargo"
	@echo -e "   make clean\n"
	@echo -e "Then you can retry building!\n"
	@exit 1
else
	@echo -e '\nFound proper xargo version, proceeding with build...\n'
endif ## RUSTC_CURRENT_SUPPORTED_VERSION != RUSTC_OUTPUT
endif ## BYPASS_XARGO_CHECK



###################################################################################################
### This section contains targets to actually build Theseus components and create an iso file.
###################################################################################################

BUILD_DIR := $(ROOT_DIR)/build
NANO_CORE_BUILD_DIR := $(BUILD_DIR)/nano_core
iso := $(BUILD_DIR)/theseus-$(ARCH).iso
GRUB_ISOFILES := $(BUILD_DIR)/grub-isofiles
OBJECT_FILES_BUILD_DIR := $(GRUB_ISOFILES)/modules


## This is the output path of the xargo command, defined by cargo (not our choice).
nano_core_static_lib := $(ROOT_DIR)/target/$(TARGET)/$(BUILD_MODE)/libnano_core.a
## The directory where the nano_core source files are
NANO_CORE_SRC_DIR := $(ROOT_DIR)/kernel/nano_core/src
## The output directory of where the nano_core binary should go
nano_core_binary := $(NANO_CORE_BUILD_DIR)/nano_core-$(ARCH).bin
## The linker script for linking the nano_core_binary to the assembly files
linker_script := $(NANO_CORE_SRC_DIR)/boot/arch_$(ARCH)/linker_higher_half.ld
assembly_source_files := $(wildcard $(NANO_CORE_SRC_DIR)/boot/arch_$(ARCH)/*.asm)
assembly_object_files := $(patsubst $(NANO_CORE_SRC_DIR)/boot/arch_$(ARCH)/%.asm, \
	$(NANO_CORE_BUILD_DIR)/boot/$(ARCH)/%.o, $(assembly_source_files))


# get all the subdirectories in kernel/, i.e., the list of all kernel crates
KERNEL_CRATE_NAMES := $(notdir $(wildcard kernel/*))
# exclude the build directory 
KERNEL_CRATE_NAMES := $(filter-out build/. target/., $(KERNEL_CRATE_NAMES))
# exclude hidden directories starting with a "."
KERNEL_CRATE_NAMES := $(filter-out .*/, $(KERNEL_CRATE_NAMES))
# remove the trailing /. on each name
KERNEL_CRATE_NAMES := $(patsubst %/., %, $(KERNEL_CRATE_NAMES))

# get all the subdirectories in applications/, i.e., the list of application crates
APP_CRATE_NAMES := $(notdir $(wildcard applications/*))
# exclude the build directory 
APP_CRATE_NAMES := $(filter-out build/. target/., $(APP_CRATE_NAMES))
# exclude hidden directories starting with a "."
APP_CRATE_NAMES := $(filter-out .*/, $(APP_CRATE_NAMES))
# remove the trailing /. on each name
APP_CRATE_NAMES := $(patsubst %/., %, $(APP_CRATE_NAMES))

## Specify which crates are app libraries. 
## These crates can be instantiated multiply (per-task) rather than once (system-wide);
## they will only be multiply instantiated if they have data/bss sections
## Ideally we would do this with a script that analyzes dependencies to see if a crate is only used by application crates,
## but I haven't had time yet to develop that script. It would be fairly straightforward using a tool like `cargo deps`. 
## So, for now, we just do it manually.
## You can execute this to view dependencies to help you out:
## `cd kernel/nano_core && cargo deps --include-orphans --no-transitive-deps | dot -Tpdf > /tmp/graph.pdf && xdg-open /tmp/graph.pdf`
APP_CRATE_NAMES += getopts unicode_width


### PHONY is the list of targets that *always* get rebuilt regardless of dependent files' modification timestamps.
### Most targets are PHONY because cargo itself handles whether or not to rebuild the Rust code base.
.PHONY: all \
		check_rustc check_xargo check_captain \
		clean run debug iso build userspace cargo \
		simd_personality_sse build_sse simd_personality_avx build_avx \
		$(assembly_source_files) \
		gdb doc docs view-doc view-docs


### If we compile for SIMD targets newer than SSE (e.g., AVX or newer),
### then we need to define a preprocessor variable 
### that will cause the AVX flag to be enabled in the boot-up assembly code. 
ifneq (,$(findstring avx, $(TARGET)))
$(eval CFLAGS += -DENABLE_AVX)
endif


### After the compilation process, check that we have exactly one captain module, which is needed for loadable mode.
NUM_CAPTAINS = $(shell ls $(OBJECT_FILES_BUILD_DIR)/$(KERNEL_PREFIX)captain-* | wc -l)
check_captain:
	@if [ 1  !=  ${NUM_CAPTAINS} ]; then \
		echo -e "\nError: there are multiple 'captain' modules in the OS image, which will cause problems after bootup."; \
		echo -e "       Run \"make clean\" and then try rebuilding again.\n"; \
		exit 1; \
	fi;


### This target builds an .iso OS image from all of the compiled crates.
### It skips building userspace for now, but you can add it back in by adding "userspace" to the line below.
$(iso): build check_captain
# after building kernel and application modules, copy the kernel boot image files
	@mkdir -p $(GRUB_ISOFILES)/boot/grub
	@cp $(nano_core_binary) $(GRUB_ISOFILES)/boot/kernel.bin
# autogenerate the grub.cfg file
	cargo run --manifest-path $(ROOT_DIR)/tools/grub_cfg_generation/Cargo.toml -- $(GRUB_ISOFILES)/modules/ -o $(GRUB_ISOFILES)/boot/grub/grub.cfg
	$(GRUB_MKRESCUE) -o $(iso) $(GRUB_ISOFILES)  2> /dev/null


### Convenience target for building the ISO	using the above target
iso: $(iso)


## This first invokes the make target that runs the actual compiler, and then copies all object files into the build dir.
## This also classifies crate object files into either "application" or "kernel" crates:
## -- an application crate is any executable application in the `applications/` directory, or a library crate that is ONLY used by other applications,
## -- a kernel crate is any crate in the `kernel/` directory, or any other crates that are used 
## Obviously, if a crate is used by both other application crates and by kernel crates, it is still a kernel crate. 
## Then, we give all kernel crate object files the KERNEL_PREFIX and all application crate object files the APP_PREFIX.
build: $(nano_core_binary)
## Copy all object files into the main build directory and prepend the kernel prefix.
## All object files include those from the target/ directory, and the core, alloc, and compiler_builtins libraries
	@for f in ./target/$(TARGET)/$(BUILD_MODE)/deps/*.o "$(HOME)"/.xargo/lib/rustlib/$(TARGET)/lib/*.o; do \
		cp -vf  $${f}  $(OBJECT_FILES_BUILD_DIR)/`basename $${f} | sed -n -e 's/\(.*\)/$(KERNEL_PREFIX)\1/p'`   2> /dev/null ; \
	done
## In the above loop, we gave all object files the kernel prefix, so we need to rename the application object files with the proper app prefix.
	@for app in $(APP_CRATE_NAMES) ; do  \
		OLD_FILE_PATH=$(OBJECT_FILES_BUILD_DIR)/$(KERNEL_PREFIX)$${app}-*.o ; \
		NEW_FILE_PATH=$(OBJECT_FILES_BUILD_DIR)/`basename $${OLD_FILE_PATH} | sed -n -e 's/$(KERNEL_PREFIX)\(.*\)/$(APP_PREFIX)\1/p'` ; \
		mv  $${OLD_FILE_PATH}  $${NEW_FILE_PATH} ; \
	done
## Strip debug information (optional, just improves QEMU load times and reduces mem usage)
	@for f in $(OBJECT_FILES_BUILD_DIR)/*.o ; do \
		$(CROSS)strip  --strip-debug  $${f} ; \
	done



## This target invokes the actual Rust build process
cargo: check_rustc check_xargo
	@echo -e "\n=================== BUILDING ALL CRATES ==================="
	@echo -e "\t TARGET: \"$(TARGET)\""
	@echo -e "\t KERNEL_PREFIX: \"$(KERNEL_PREFIX)\""
	@echo -e "\t APP_PREFIX: \"$(APP_PREFIX)\""
	@echo -e "\t THESEUS_CONFIG (before build.rs script): \"$(THESEUS_CONFIG)\""
	RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" xargo build  $(CARGOFLAGS)  $(RUST_FEATURES) --all --target $(TARGET)

## We tried using the "xargo rustc" command here instead of "xargo build" to avoid xargo unnecessarily rebuilding core/alloc crates,
## But it doesn't really seem to work (it's not the cause of xargo rebuilding everything).
## For the "xargo rustc" command below, all of the arguments to cargo/xargo come before the "--",
## whereas all of the arguments to rustc come after the "--".
# 	for kd in $(KERNEL_CRATE_NAMES) ; do  \
# 		cd $${kd} ; \
# 		echo -e "\n========= BUILDING KERNEL CRATE $${kd} ==========\n" ; \
# 		RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" \
# 			xargo rustc \
# 			$(CARGOFLAGS) \
# 			$(RUST_FEATURES) \
# 			--target $(TARGET) ; \
# 		cd .. ; \
# 	done
# for app in $(APP_CRATE_NAMES) ; do  \
# 	cd $${app} ; \
# 	RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" \
# 		xargo rustc \
# 		$(CARGOFLAGS) \
# 		--target $(TARGET) \
# 		-- \
# 		$(COMPILER_LINTS) ; \
# 	cd .. ; \
# done


## This builds the nano_core binary itself, which is the fully-linked code that first runs right after the bootloader
$(nano_core_binary): cargo $(nano_core_static_lib) $(assembly_object_files) $(linker_script)
	@mkdir -p $(BUILD_DIR)
	@mkdir -p $(NANO_CORE_BUILD_DIR)
	@mkdir -p $(OBJECT_FILES_BUILD_DIR)
	$(CROSS)ld -n -T $(linker_script) -o $(nano_core_binary) $(assembly_object_files) $(nano_core_static_lib)
## run "readelf" on the nano_core binary, remove non-FUNC LOCAL symbols and WEAK symbols from the ELF file, and then demangle it, and then output to a sym file
	@cargo run --manifest-path $(ROOT_DIR)/tools/demangle_readelf_file/Cargo.toml \
		<($(CROSS)readelf -S -s -W $(nano_core_binary) | sed '/OBJECT  LOCAL  /d;/NOTYPE  LOCAL  /d;/FILE    LOCAL  /d;/SECTION LOCAL  /d;/WEAK   /d') \
		>  $(OBJECT_FILES_BUILD_DIR)/$(KERNEL_PREFIX)nano_core.sym
	@echo -n -e '\0' >> $(OBJECT_FILES_BUILD_DIR)/$(KERNEL_PREFIX)nano_core.sym


### This compiles the assembly files in the nano_core. 
### This target is currently rebuilt every time to accommodate changing CFLAGS.
$(NANO_CORE_BUILD_DIR)/boot/$(ARCH)/%.o: $(NANO_CORE_SRC_DIR)/boot/arch_$(ARCH)/%.asm
	@mkdir -p $(shell dirname $@)
	@nasm -felf64 $< -o $@ $(CFLAGS)



## (This is currently not used in Theseus, since we don't run anything in userspace)
## This builds all userspace programs
userspace: 
	@echo -e "\n======== BUILDING USERSPACE ========"
	@$(MAKE) -C userspace all
## copy userspace binary files and add the __u_ prefix
	@mkdir -p $(GRUB_ISOFILES)/modules
	@for f in `find $(ROOT_DIR)/userspace/build -type f` ; do \
		cp -vf $${f}  $(GRUB_ISOFILES)/modules/`basename $${f} | sed -n -e 's/\(.*\)/__u_\1/p'` 2> /dev/null ; \
	done



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
## It builds everything with the SIMD-enabled x86_64-theseus-sse target,
## and then builds everything again with the regular x86_64-theseus target. 
## The "normal" target must come last ('build_sse', THEN the regular 'build') to ensure that the final nano_core_binary is non-SIMD.
simd_personality_sse : export TARGET := x86_64-theseus
simd_personality_sse : export BUILD_MODE = release
simd_personality_sse : export override THESEUS_CONFIG += simd_personality
simd_personality_sse: build_sse build
## after building all the modules, copy the kernel boot image files
	@echo -e "********* AT THE END OF SIMD_BUILD: TARGET = $(TARGET), KERNEL_PREFIX = $(KERNEL_PREFIX), APP_PREFIX = $(APP_PREFIX)"
	@mkdir -p $(GRUB_ISOFILES)/boot/grub
	@cp $(nano_core_binary) $(GRUB_ISOFILES)/boot/kernel.bin
## autogenerate the grub.cfg file
	cargo run --manifest-path $(ROOT_DIR)/tools/grub_cfg_generation/Cargo.toml -- $(GRUB_ISOFILES)/modules/ -o $(GRUB_ISOFILES)/boot/grub/grub.cfg
	@$(GRUB_MKRESCUE) -o $(iso) $(GRUB_ISOFILES)  2> /dev/null
## run it in QEMU
	qemu-system-x86_64 $(QEMU_FLAGS)



## This target is like "simd_personality_sse", but uses AVX instead of SSE.
## It builds everything with the SIMD-enabled x86_64-theseus-avx target,
## and then builds everything again with the regular x86_64-theseus target. 
## The "normal" target must come last ('build_avx', THEN the regular 'build') to ensure that the final nano_core_binary is non-SIMD.
simd_personality_avx : export TARGET := x86_64-theseus
simd_personality_avx : export BUILD_MODE = release
simd_personality_avx : export override THESEUS_CONFIG += simd_personality
simd_personality_avx : export override CFLAGS += -DENABLE_AVX
simd_personality_avx: build_avx build
## after building all the modules, copy the kernel boot image files
	@echo -e "********* AT THE END OF SIMD_BUILD: TARGET = $(TARGET), KERNEL_PREFIX = $(KERNEL_PREFIX), APP_PREFIX = $(APP_PREFIX)"
	@mkdir -p $(GRUB_ISOFILES)/boot/grub
	@cp $(nano_core_binary) $(GRUB_ISOFILES)/boot/kernel.bin
## autogenerate the grub.cfg file
	cargo run --manifest-path $(ROOT_DIR)/tools/grub_cfg_generation/Cargo.toml -- $(GRUB_ISOFILES)/modules/ -o $(GRUB_ISOFILES)/boot/grub/grub.cfg
	@$(GRUB_MKRESCUE) -o $(iso) $(GRUB_ISOFILES)  2> /dev/null
## run it in QEMU
	qemu-system-x86_64 $(QEMU_FLAGS)



### build_sse builds the kernel and applications with the x86_64-theseus-sse target.
### It can serve as part of the simd_personality_sse target.
build_sse : export TARGET := x86_64-theseus-sse
build_sse : export override RUSTFLAGS += -C no-vectorize-loops
build_sse : export override RUSTFLAGS += -C no-vectorize-slp
build_sse : export KERNEL_PREFIX := ksse\#
build_sse : export APP_PREFIX := asse\#
build_sse:
	@$(MAKE) build


### build_avx builds the kernel and applications with the x86_64-theseus-avx target.
### It can serve as part of the simd_personality_avx target.
build_avx : export TARGET := x86_64-theseus-avx
build_avx : export override RUSTFLAGS += -C no-vectorize-loops
build_avx : export override RUSTFLAGS += -C no-vectorize-slp
build_avx : export KERNEL_PREFIX := kavx\#
build_avx : export APP_PREFIX := aavx\#
build_avx:
	@$(MAKE) build



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



## The top-level (root) documentation file
DOC_ROOT := $(ROOT_DIR)/build/doc/___Theseus_Crates___/index.html

## Builds Theseus's documentation.
## The entire project is built as normal using the "cargo doc" command.
docs: doc
doc: check_rustc
	@cargo doc --all --no-deps $(addprefix --exclude , $(APP_CRATE_NAMES))
	@rustdoc --output target/doc --crate-name "___Theseus_Crates___" $(ROOT_DIR)/kernel/_doc_root.rs
	@mkdir -p build
	@rm -rf build/doc
	@cp -rf target/doc ./build/
	@echo -e "\nDocumentation is now available at: \"$(DOC_ROOT)\"."


## Opens the documentation root in the system's default browser. 
## the "powershell" command is used on Windows Subsystem for Linux
view-docs: view-doc
view-doc: doc
	@echo -e "Opening documentation index file in your browser..."
ifneq ($(IS_WSL), )
## building on WSL
	@cmd.exe /C start "$(shell wslpath -w $(DOC_ROOT))" &
else
## building on regular Linux or macOS
	@xdg-open $(DOC_ROOT) > /dev/null 2>&1 || open $(DOC_ROOT) &
endif


## The top-level book file.
BOOK_ROOT := $(ROOT_DIR)/book/book/index.html

## Builds the Theseus book in the `book` directory.
book: $(wildcard book/src/*)
ifneq ($(shell mdbook --version > /dev/null 2>&1 && echo $$?), 0)
	@echo -e "\nError: please install mdbook:"
	@echo -e "  cargo install mdbook"
	@exit 1
endif
	@cd book && mdbook build
	@echo -e "\nThe Theseus Book is now available at \"$(BOOK_ROOT)\"."


## Opens the top-level file of the Theseus book.
view-book: book


## Removes all build files
clean:
	cargo clean
	@rm -rf build
#@$(MAKE) -C userspace clean
	


help: 
	@echo -e "\nThe following make targets are available:"
	@echo -e "   iso:"
	@echo -e "\t The default and most basic target. Builds the full Theseus OS and creates a bootable ISO image."

	@echo -e "   run:"
	@echo -e "\t Builds Theseus (like the 'iso' target) and runs it using QEMU."

	@echo -e "   loadable:"
	@echo -e "\t Same as 'run', but enables the 'loadable' configuration so that all crates are dynamically loaded."

	@echo -e "   debug:"
	@echo -e "\t Same as 'run', but pauses QEMU at its GDB stub entry point,"
	@echo -e "\t which waits for you to connect a GDB debugger using 'make gdb'."

	@echo -e "   gdb:"
	@echo -e "\t Runs a new instance of GDB that connects to an already-running QEMU instance."
	@echo -e "\t You must run 'make debug' beforehand in a separate terminal."

	@echo -e "   bochs:"
	@echo -e "\t Same as 'make run', but runs Theseus in the Bochs emulator instead of QEMU."

	@echo -e "   boot:"
	@echo -e "\t Builds Theseus as a bootable .iso and writes it to the specified USB drive."
	@echo -e "\t The USB drive is specified as usb=<dev-name>, e.g., 'make boot usb=sdc',"
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
	
	@echo -e "\nThe following key-value options are available for QEMU targets, like 'run' and 'debug':"
	@echo -e "   net=user:"
	@echo -e "\t Enable networking with an e1000 NIC in the guest and a userspace SLIRP-based interface in the host (QEMU default)."
	@echo -e "   net=tap:"
	@echo -e "\t Enable networking with an e1000 NIC in the guest and a TAP interface in the host."
# @echo -e "   kvm=yes:"
# @echo -e "\t Enable KVM acceleration (the host computer must support it)."
	@echo -e "   host=yes:"
	@echo -e "\t Use the host CPU model, which is required for using certain x86 hardware, e.g., PMU, AVX. This also enables KVM."
	@echo -e "   int=yes:"
	@echo -e "\t Enable interrupt logging in QEMU console (-d int)."

	@echo -e "\nThe following make targets exist for building documentation:"
	@echo -e "   doc:"
	@echo -e "\t Builds Theseus documentation from its Rust source code (rustdoc)."
	@echo -e "   view-doc:"
	@echo -e "\t Builds Theseus documentation and then opens it in your default browser."
	@echo -e "   book:"
	@echo -e "\t Builds the Theseus book using the mdbook Markdown tool."
	@echo -e "   view-book:"
	@echo -e "\t Builds the Theseus book and then opens it in your default browser."
	@echo ""






###################################################################################################
### This section has QEMU arguments and configuration
###################################################################################################
QEMU_MEMORY ?= 512M
QEMU_FLAGS := -cdrom $(iso) -no-reboot -no-shutdown -s -m $(QEMU_MEMORY) -serial stdio 
## multicore 
QEMU_FLAGS += -smp 4

## QEMU's OUI dictates that the MAC addr start with "52:54:00:"
MAC_ADDR ?= 52:54:00:d1:55:01

## Add a disk drive, a PATA drive over an IDE controller interface.
QEMU_FLAGS += -drive format=raw,file=random_data2.img,if=ide

## Add a disk drive, a SATA drive over the AHCI interface.
## We don't yet have SATA support in Theseus.
# QEMU_FLAGS += -drive id=my_disk,file=random_data2.img,if=none  -device ahci,id=ahci  -device ide-drive,drive=my_disk,bus=ahci.0

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
else
	QEMU_FLAGS += -net none
endif

## Dump interrupts to the serial port log
ifeq ($(int),yes)
	QEMU_FLAGS += -d int
endif

ifeq ($(host),yes)
	## KVM acceleration is required when using the host cpu model
	QEMU_FLAGS += -cpu host -accel kvm
else
	QEMU_FLAGS += -cpu Broadwell
endif

## Currently, kvm by itself can cause problems, but it works with the "host" option (above).
ifeq ($(kvm),yes)
$(error Error: the 'kvm=yes' option is currently broken. Use 'host=yes' instead")
	# QEMU_FLAGS += -accel kvm
endif



###################################################################################################
### This section has targets for running and debugging 
###################################################################################################

### Old Run: runs the most recent build without rebuilding
orun:
	@qemu-system-x86_64 $(QEMU_FLAGS)


### Old Debug: runs the most recent build with debugging without rebuilding
odebug:
	@qemu-system-x86_64 $(QEMU_FLAGS) -S


### Currently, loadable module mode requires release build mode
loadable : export override THESEUS_CONFIG += loadable
loadable : export BUILD_MODE = release
loadable: run


### builds and runs Theseus in QEMU
run: $(iso) 
	# @qemu-img resize random_data2.img 100K
	qemu-system-x86_64 $(QEMU_FLAGS)


### builds and runs Theseus in QEMU, but pauses execution until a GDB instance is connected.
debug: $(iso)
	@qemu-system-x86_64 $(QEMU_FLAGS) -S
#-monitor stdio


### Runs a gdb instance on the host machine. 
### Run this after invoking "make debug" in a different terminal.
gdb:
	@rust-os-gdb/bin/rust-gdb "$(nano_core_binary)" -ex "target remote :1234"



### builds and runs Theseus in Bochs
bochs : export override THESEUS_CONFIG += apic_timer_fixed
bochs: $(iso) 
	# @qemu-img resize random_data2.img 100K
	bochs -f bochsrc.txt -q




### Checks that the supplied usb device (for usage with the boot/pxe targets).
### Note: this is bypassed on WSL, because WSL doesn't support raw device files yet.
check_usb:
## on WSL, we bypass the check for USB, because burning the ISO to USB must be done with a Windows app.
ifeq ($(IS_WSL), ) ## if we're not on WSL...
## now we need to check that the user has specified a USB drive that actually exists, not a partition of a USB drive.
ifeq (,$(findstring $(usb),$(USB_DRIVES)))
	@echo -e "\nError: please specify a USB drive that exists, e.g., \"sdc\" (not a partition like \"sdc1\")."
	@echo -e "For example, run the following command:"
	@echo -e "   make boot usb=sdc\n"
	@echo -e "The following USB drives are currently attached to this system:\n$(USB_DRIVES)"
	@echo ""
	@exit 1
endif  ## end of checking that the 'usb' variable is a USB drive that exists
endif  ## end of checking for WSL


### Creates a bootable USB drive that can be inserted into a real PC based on the compiled .iso. 
boot : export override THESEUS_CONFIG += mirror_log_to_vga
boot: check_usb $(iso)
ifneq ($(IS_WSL), )
## building on WSL
	@echo -e "\n\033[1;32mThe build finished successfully\033[0m, but WSL is unable to access raw USB devices. Instead, you must burn the ISO to a USB drive yourself."
	@echo -e "The ISO file is available at \"$(iso)\"."
else
## building on Linux or macOS
	@$(UNMOUNT) /dev/$(usb)* 2> /dev/null  |  true  ## force it to return true
	@sudo dd bs=4194304 if=$(iso) of=/dev/$(usb)    ## use 4194304 instead of 4M because macOS doesn't support 4M
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
