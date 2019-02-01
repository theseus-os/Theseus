### This makefile is the top-level build script that builds all the crates in subdirectories 
### and combines them into the final OS .iso image.
### It also provides convenient targets for running and debugging Theseus and using GDB on your host computer.
.DEFAULT_GOAL := all
SHELL := /bin/bash

## most of the variables used below are defined in Config.mk
include cfg/Config.mk

.PHONY: all check_rustc check_xargo check_captain clean run debug iso build userspace cargo simd_personality build_simd gdb doc docs view-doc view-docs

all: iso


###################################################################################################
### For ensuring that the host computer has the proper version of the Rust compiler
###################################################################################################

RUSTC_CURRENT_SUPPORTED_VERSION := rustc 1.32.0-nightly (f1e2fa8f0 2018-11-20)
RUSTC_CURRENT_INSTALL_VERSION := nightly-2018-11-21
RUSTC_OUTPUT=$(shell rustc --version)

check_rustc: 	
ifneq (${BYPASS_RUSTC_CHECK}, yes)
ifneq (${RUSTC_CURRENT_SUPPORTED_VERSION}, ${RUSTC_OUTPUT})
	@echo -e "\nError: your rustc version does not match our supported compiler version."
	@echo -e "To install the proper version of rustc, run the following commands:\n"
	@echo -e "   rustup toolchain install $(RUSTC_CURRENT_INSTALL_VERSION)"
	@echo -e "   rustup default $(RUSTC_CURRENT_INSTALL_VERSION)"
	@echo -e "   rustup component add rust-src"
	@echo -e "   make clean\n"
	@echo -e "Then you can retry building!\n"
	@exit 1
else
	@echo -e '\nFound proper rust compiler version, proceeding with build...\n'
endif ## RUSTC_CURRENT_SUPPORTED_VERSION != RUSTC_OUTPUT
endif ## BYPASS_RUSTC_CHECK



###################################################################################################
### For ensuring that the host computer has the proper version of xargo
###################################################################################################

XARGO_CURRENT_SUPPORTED_VERSION := 0.3.12
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

BUILD_DIR := build
NANO_CORE_BUILD_DIR := $(BUILD_DIR)/nano_core
iso := $(BUILD_DIR)/theseus-$(ARCH).iso
GRUB_ISOFILES := $(BUILD_DIR)/grub-isofiles
OBJECT_FILES_BUILD_DIR := $(GRUB_ISOFILES)/modules


## This is the output path of the xargo command, defined by cargo (not our choice).
nano_core_static_lib := target/$(TARGET)/$(BUILD_MODE)/libnano_core.a
## The directory where the nano_core source files are
NANO_CORE_SRC_DIR := kernel/nano_core/src
## The output directory of where the nano_core binary should go
nano_core_binary := $(NANO_CORE_BUILD_DIR)/nano_core-$(ARCH).bin
## The linker script for linking the nano_core_binary to the assembly files
linker_script := $(NANO_CORE_SRC_DIR)/boot/arch_$(ARCH)/linker_higher_half.ld
assembly_source_files := $(wildcard $(NANO_CORE_SRC_DIR)/boot/arch_$(ARCH)/*.asm)
assembly_object_files := $(patsubst $(NANO_CORE_SRC_DIR)/boot/arch_$(ARCH)/%.asm, \
	$(NANO_CORE_BUILD_DIR)/boot/$(ARCH)/%.o, $(assembly_source_files))


# get all the subdirectories in kernel/, i.e., the list of all kernel crates
KERNEL_CRATES := $(notdir $(wildcard kernel/*))
# exclude the build directory 
KERNEL_CRATES := $(filter-out build/. target/., $(KERNEL_CRATES))
# exclude hidden directories starting with a "."
KERNEL_CRATES := $(filter-out .*/, $(KERNEL_CRATES))
# remove the trailing /. on each name
KERNEL_CRATES := $(patsubst %/., %, $(KERNEL_CRATES))

# get all the subdirectories in applications/, i.e., the list of application crates
APP_CRATES := $(notdir $(wildcard applications/*))
# exclude the build directory 
APP_CRATES := $(filter-out build/. target/., $(APP_CRATES))
# exclude hidden directories starting with a "."
APP_CRATES := $(filter-out .*/, $(APP_CRATES))
# remove the trailing /. on each name
APP_CRATES := $(patsubst %/., %, $(APP_CRATES))



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
	cargo run --manifest-path tools/grub_cfg_generation/Cargo.toml -- $(GRUB_ISOFILES)/modules/ -o $(GRUB_ISOFILES)/boot/grub/grub.cfg
	@grub-mkrescue -o $(iso) $(GRUB_ISOFILES)  2> /dev/null


### Convenience target for building the ISO	using the above target
iso: $(iso)


## This first invokes the make target that runs the actual compiler, and then copies all object files into the build dir. 
## It gives all object files the KERNEL_PREFIX, except for "executable" application object files that get the APP_PREFIX.
build: $(nano_core_binary)
## Copy the object files from the target/ directory, and the core library, into the main build directory and prepend the kernel prefix
	@for f in ./target/$(TARGET)/$(BUILD_MODE)/deps/*.o $(HOME)/.xargo/lib/rustlib/$(TARGET)/lib/core-*.o; do \
		cp -vf  $${f}  $(OBJECT_FILES_BUILD_DIR)/`basename $${f} | sed -n -e 's/\(.*\)/$(KERNEL_PREFIX)\1/p'`   2> /dev/null ; \
	done

## Above, we gave all object files the kernel prefix, so we need to rename the application object files with the proper app prefix
## Currently, we remove the hash suffix from application object file names so they're easier to find, but we could change that later 
##            if we ever want to give applications specific versioning semantics (based on those hashes, like with kernel crates)
	@for app in $(APP_CRATES) ; do  \
		mv  $(OBJECT_FILES_BUILD_DIR)/$(KERNEL_PREFIX)$${app}-*.o  $(OBJECT_FILES_BUILD_DIR)/$(APP_PREFIX)$${app}.o ; \
		strip --strip-debug  $(OBJECT_FILES_BUILD_DIR)/$(APP_PREFIX)$${app}.o ; \
	done



## This target invokes the actual Rust build process
cargo: check_rustc check_xargo
	@echo -e "\n=================== BUILDING ALL CRATES ==================="
	@echo -e "\t TARGET: \"$(TARGET)\""
	@echo -e "\t KERNEL_PREFIX: \"$(KERNEL_PREFIX)\""
	@echo -e "\t APP_PREFIX: \"$(APP_PREFIX)\""
	@echo -e "\t THESEUS_CONFIG: \"$(THESEUS_CONFIG)\""
	RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" xargo build  $(CARGO_OPTIONS)  $(RUST_FEATURES) --all --target $(TARGET)

## We tried using the "xargo rustc" command here instead of "xargo build" to avoid xargo unnecessarily rebuilding core/alloc crates,
## But it doesn't really seem to work (it's not the cause of xargo rebuilding everything).
## For the "xargo rustc" command below, all of the arguments to cargo/xargo come before the "--",
## whereas all of the arguments to rustc come after the "--".
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
# for app in $(APP_CRATES) ; do  \
# 	cd $${app} ; \
# 	RUST_TARGET_PATH="$(CFG_DIR)" RUSTFLAGS="$(RUSTFLAGS)" \
# 		xargo rustc \
# 		$(CARGO_OPTIONS) \
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
	ld -n -T $(linker_script) -o $(nano_core_binary) $(assembly_object_files) $(nano_core_static_lib)
## run "readelf" on the nano_core binary, remove LOCAL and WEAK symbols from the ELF file, and then demangle it, and then output to a sym file
	cargo run --manifest-path $(ROOT_DIR)/tools/demangle_readelf_file/Cargo.toml \
		<(readelf -S -s -W $(nano_core_binary) | sed '/LOCAL  /d;/WEAK   /d')  >  $(OBJECT_FILES_BUILD_DIR)/$(KERNEL_PREFIX)nano_core.sym


## This compiles the assembly files in the nano_core
$(NANO_CORE_BUILD_DIR)/boot/$(ARCH)/%.o: $(NANO_CORE_SRC_DIR)/boot/arch_$(ARCH)/%.asm
	@mkdir -p $(shell dirname $@)
	@nasm -felf64 $< -o $@



## (This is currently not used in Theseus, since we don't run anything in userspace)
## This builds all userspace programs
userspace: 
	@echo -e "\n======== BUILDING USERSPACE ========"
	@$(MAKE) -C userspace all
## copy userspace binary files and add the __u_ prefix
	@mkdir -p $(GRUB_ISOFILES)/modules
	@for f in `find ./userspace/build -type f` ; do \
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
# 	@cd $(COMPILER_PLUGINS_DIR) && cargo build $(CARGO_OPTIONS)




## "simd_personality" is a special target that enables SIMD personalities.
## This builds everything with the SIMD-enabled x86_64-theseus-sse target,
## and then builds everything again with the regular x86_64-theseus target. 
## The "normal" target must come last ('build_simd', THEN the regular 'build') to ensure that the final nano_core_binary is non-SIMD.
simd_personality : export TARGET := x86_64-theseus
simd_personality : export BUILD_MODE = release
simd_personality : export THESEUS_CONFIG += simd_personality
simd_personality: build_simd build
## after building all the modules, copy the kernel boot image files
	@echo -e "********* AT THE END OF SIMD_BUILD: TARGET = $(TARGET), KERNEL_PREFIX = $(KERNEL_PREFIX), APP_PREFIX = $(APP_PREFIX)"
	@mkdir -p $(GRUB_ISOFILES)/boot/grub
	@cp $(nano_core_binary) $(GRUB_ISOFILES)/boot/kernel.bin
## autogenerate the grub.cfg file
	cargo run --manifest-path tools/grub_cfg_generation/Cargo.toml -- $(GRUB_ISOFILES)/modules/ -o $(GRUB_ISOFILES)/boot/grub/grub.cfg
	@grub-mkrescue -o $(iso) $(GRUB_ISOFILES)  2> /dev/null
## run it in QEMU
	qemu-system-x86_64 $(QEMU_FLAGS)


### build_simd is an internal target that builds the kernel and applications with the x86_64-theseus-sse target.
### It is the latter half of the simd_personality target.
build_simd : export TARGET := x86_64-theseus-sse
build_simd : export RUSTFLAGS += -C no-vectorize-loops
build_simd : export RUSTFLAGS += -C no-vectorize-slp
build_simd : export KERNEL_PREFIX := k_sse\#
build_simd : export APP_PREFIX := a_sse\#
build_simd:
## now we build the full OS again with SIMD support enabled (it has already been built normally in the "build" target)
	@echo -e "\n======== BUILDING SIMD KERNEL, TARGET = $(TARGET), KERNEL_PREFIX = $(KERNEL_PREFIX), APP_PREFIX = $(APP_PREFIX) ========"
	@$(MAKE) build



## The top-level (root) documentation file
DOC_ROOT := "build/doc/___Theseus_Crates___/index.html"

## Builds Theseus's documentation.
## The entire project is built as normal using the "cargo doc" command.
doc: check_rustc
	@cargo doc --no-deps
	@rustdoc --output target/doc --crate-name "___Theseus_Crates___" kernel/documentation/src/_top.rs
	@mkdir -p build
	@rm -rf build/doc
	@cp -rf target/doc ./build/
	@echo -e "\n\nDocumentation is now available in the build/doc directory."
	@echo -e "You can also run 'make view-doc' to view it."

docs: doc


## Opens the documentation root in the system's default browser. 
## the "powershell" command is used on Windows Subsystem for Linux
view-doc: doc
	@xdg-open $(DOC_ROOT) > /dev/null 2>&1 || powershell.exe -c $(DOC_ROOT) &

view-docs: view-doc


## Removes all build files
clean:
	cargo clean
	@rm -rf build
	@$(MAKE) -C userspace clean
	


help: 
	@echo -e "\nThe following make targets are available:"
	@echo -e "   run:"
	@echo -e "\t The most common target. Builds and runs Theseus using the QEMU emulator."
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
	@echo -e "   simd_personality:"
	@echo -e "\t Builds Theseus with a regular personality and a SIMD-enabled personality,"
	@echo -e "\t then runs it just like the 'make run' target."
	@echo -e "   doc:"
	@echo -e "\t Builds Theseus documentation from its Rust source code (rustdoc)."
	@echo -e "   view-doc:"
	@echo -e "\t Builds Theseus documentation and then opens it in your default browser."
	
	@echo -e "\nThe following key-value options are available for QEMU targets, like 'run' and 'debug':"
	@echo -e "   net=yes:"
	@echo -e "\t Enable networking with an e1000 NIC in the guest and a TAP interface in the host."
	@echo -e "   kvm=yes:"
	@echo -e "\t Enable KVM acceleration (the host computer must support it)."
	@echo -e "   host=yes:"
	@echo -e "\t Use the host CPU model, which is required for using x86 PMU hardware and others. Enables KVM too."
	@echo -e "   int=yes:"
	@echo -e "\t Enable interrupt logging in QEMU console (-d int)."
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

## drive and devices commands from http://forum.osdev.org/viewtopic.php?f=1&t=26483 to use sata emulation
QEMU_FLAGS += -drive format=raw,file=random_data2.img,if=none,id=mydisk -device ide-hd,drive=mydisk,bus=ide.0,serial=4696886396

ifeq ($(net),yes)
	## Read about QEMU networking options here: https://www.qemu.org/2018/05/31/nic-parameter/
	
	## basic userspace QEMU networking with a standard e1000 ethernet NIC
	#QEMU_FLAGS += -net nic,vlan=0,model=e1000,macaddr=00:0b:82:01:fc:42 -net dump,file=netdump.pcap
	#QEMU_FLAGS += -net nic,vlan=1,model=e1000 -net user,vlan=1 -net dump,file=netdump.pcap

	## TAP-based networking setup with a standard e1000 ethernet NIC frontent (in the guest) and the TAP backend (in the host)
	QEMU_FLAGS += -device e1000,netdev=network0,mac=$(MAC_ADDR) -netdev tap,id=network0,ifname=tap0,script=no,downscript=no
	QEMU_FLAGS += -object filter-dump,id=f1,netdev=network0,file=netdump.pcap
else
	QEMU_FLAGS += -net none
endif

ifeq ($(int),yes)
	QEMU_FLAGS += -d int
endif

ifeq ($(host),yes)
	QEMU_FLAGS += -cpu host -enable-kvm
else
	QEMU_FLAGS += -cpu Broadwell
endif

ifeq ($(kvm),yes)
	QEMU_FLAGS += -enable-kvm
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
loadable : export THESEUS_CONFIG += loadable
loadable : export BUILD_MODE = release
loadable: run

### Create a make prioirty option to build and run priority scheduler
priority : export override THESEUS_CONFIG += priority_scheduler
priority : export BUILD_MODE = release
priority: run


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
bochs : export THESEUS_CONFIG += apic_timer_fixed
bochs: $(iso) 
	# @qemu-img resize random_data2.img 100K
	bochs -f bochsrc.txt -q



IS_WSL = $(shell grep 'Microsoft' /proc/version)

### Checks that the supplied usb device (for usage with the boot/pxe targets).
### Note: this is bypassed on WSL, because WSL doesn't support raw device files yet.
check_usb:
## on WSL, we bypass the check for USB, because burning the ISO to USB must be done with a Windows app
ifeq ($(IS_WSL), ) ## if we're not on WSL...
ifneq (,$(findstring sd, $(usb))) ## if the specified USB device properly contained "sd"...
ifeq ("$(wildcard /dev/$(usb))", "") ## if a non-existent "/dev/sd*" drive was specified...
	@echo -e "\nError: you specified usb drive /dev/$(usb), which does not exist.\n"
	@exit 1
endif 
else 
## if the specified USB device didn't contain "sd", then it wasn't a proper removable block device.
	@echo -e "\nError: you need to specify a usb drive, e.g., \"sdc\"."
	@echo -e "For example, run the following command:"
	@echo -e "   make boot usb=sdc\n"
	@echo -e "The following usb drives are currently attached to this system:"
	@lsblk -O | grep -i usb | awk '{print $$2}' | grep --color=never '[^0-9]$$'  # must escape '$' in makefile with '$$'
	@echo ""
	@exit 1
endif  ## end of checking for "sd"
endif  ## end of checking for WSL


### Creates a bootable USB drive that can be inserted into a real PC based on the compiled .iso. 
boot : export THESEUS_CONFIG += mirror_log_to_vga
boot: check_usb $(iso)
ifneq ($(IS_WSL), )
## building on WSL
	@echo -e "\n\033[1;32mThe build finished successfully\033[0m, but WSL is unable to access raw USB devices. Instead, you must burn the ISO to a USB drive yourself."
	@echo -e "The ISO file is available at \"$(iso)\"."
else
## building on regular linux
	@umount /dev/$(usb)* 2> /dev/null  |  true  # force it to return true
	@sudo dd bs=4M if=$(iso) of=/dev/$(usb)
	@sync
endif
	

### this builds an ISO and copies it into the theseus tftpboot folder as described in the REAEDME 
pxe : export THESEUS_CONFIG += mirror_log_to_vga
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
