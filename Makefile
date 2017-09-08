### This makefile is the top-level build script that builds all the crates in subdirectories 
### and combines them into the final OS .iso image.
### It also provides convenient targets for running and debugging Theseus and using GDB on your host computer.
.DEFAULT_GOAL := run
SHELL := /bin/bash

.PHONY: all clean run debug iso userspace cargo gdb


arch ?= x86_64
target ?= $(arch)-theseus
nano-core := kernel/nano-core/build/nano-core-$(arch).bin
iso := build/theseus-$(arch).iso
grub_cfg := cfg/grub.cfg



all: kernel userspace


###################################################################################################
### For ensuring that the host computer has the proper version of the Rust compiler
###################################################################################################

RUSTC_CURRENT_SUPPORTED_VERSION := rustc 1.21.0-nightly (c417ee9ae 2017-07-25)
RUSTC_CURRENT_INSTALL_VERSION := nightly-2017-07-26
RUSTC_OUTPUT=$(shell rustc --version)

test_rustc: 	
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
endif


###################################################################################################
### This section has QEMU arguments and configuration
###################################################################################################

QEMU_MEMORY ?= 1G
QEMU_FLAGS := -cdrom $(iso) -no-reboot -no-shutdown -s -m $(QEMU_MEMORY) -serial stdio -cpu Haswell -net none

#drive and devices commands from http://forum.osdev.org/viewtopic.php?f=1&t=26483 to use sata emulation
QEMU_FLAGS += -drive format=raw,file=random_data2.img,if=none,id=mydisk -device ide-hd,drive=mydisk,bus=ide.0,serial=4696886396 

ifeq ($(int),yes)
	QEMU_FLAGS += -d int
endif
ifeq ($(kvm),yes)
#### We're disabling KVM for the time being because it breaks some features, like RDMSR used for TSC
	#QEMU_FLAGS += -enable-kvm
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


### builds and runs theseus
run: $(iso) 
	@qemu-img resize random_data2.img 100K
	qemu-system-x86_64 $(QEMU_FLAGS)


### builds and runs theseus, but pauses execution until a GDB instance is connected.
debug: $(iso)
	@qemu-system-x86_64 $(QEMU_FLAGS) -S
#-monitor stdio


### Runs a gdb instance on the host machine. 
### Run this after invoking "make debug" in a different terminal.
gdb:
	@rust-os-gdb/bin/rust-gdb "$(nano-core)" -ex "target remote :1234"
### TODO: add more symbol files besides nano-core once they're split from nano-core



###################################################################################################
### This section contains targets to actually build Theseus components and create an iso file.
###################################################################################################

iso: $(iso)
grub-isofiles := build/grub-isofiles

### This target builds an .iso OS image from the userspace and kernel.
$(iso): kernel userspace $(grub_cfg)
	@rm -rf $(grub-isofiles) 
### copy userspace build files
	@mkdir -p $(grub-isofiles)/modules
	@cp userspace/build/* $(grub-isofiles)/modules/
### copy kernel build files
	@mkdir -p $(grub-isofiles)/boot/grub
	@cp $(nano-core) $(grub-isofiles)/boot/kernel.bin
	@cp $(grub_cfg) $(grub-isofiles)/boot/grub
	@grub-mkrescue -o $(iso) $(grub-isofiles)  # 2> /dev/null
	


### this builds all userspace programs
userspace: 
	@echo "======== BUILDING USERSPACE ========"
	@$(MAKE) -C userspace all


### this builds all kernel components
kernel: test_rustc
	@echo "======== BUILDING KERNEL ========"
	@$(MAKE) -C kernel all



clean:
	@rm -rf build
	@$(MAKE) -C kernel clean
	@$(MAKE) -C userspace clean
	

