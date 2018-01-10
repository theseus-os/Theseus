### This makefile is the top-level build script that builds all the crates in subdirectories 
### and combines them into the final OS .iso image.
### It also provides convenient targets for running and debugging Theseus and using GDB on your host computer.
.DEFAULT_GOAL := all
SHELL := /bin/bash

.PHONY: all clean run debug iso userspace cargo gdb


arch ?= x86_64
target ?= $(arch)-theseus
nano_core := kernel/nano_core/build/nano_core-$(arch).bin
iso := build/theseus-$(arch).iso
grub_cfg := cfg/grub.cfg

ifeq ($(bypass),yes)
	BYPASS_RUSTC_CHECK := yes
endif


all: iso


###################################################################################################
### For ensuring that the host computer has the proper version of the Rust compiler
###################################################################################################

RUSTC_CURRENT_SUPPORTED_VERSION := rustc 1.24.0-nightly (5a2465e2b 2017-12-06)
RUSTC_CURRENT_INSTALL_VERSION := nightly-2017-12-07
RUSTC_OUTPUT=$(shell rustc --version)

test_rustc: 	
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
### This section has QEMU arguments and configuration
###################################################################################################

QEMU_MEMORY ?= 512M
QEMU_FLAGS := -cdrom $(iso) -no-reboot -no-shutdown -s -m $(QEMU_MEMORY) -serial stdio 
## QEMU_FLAGS += -cpu Haswell
QEMU_FLAGS += -cpu Broadwell
QEMU_FLAGS += -net none
QEMU_FLAGS += -smp 2

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
	@rust-os-gdb/bin/rust-gdb "$(nano_core)" -ex "target remote :1234"
### TODO: add more symbol files besides nano_core once they're split from nano_core


check_usb:
ifneq (,$(findstring sd, $(usb)))
ifeq ("$(wildcard /dev/$(usb))", "")
	@echo -e "\nError: you specified usb drive /dev/$(usb), which does not exist.\n"
	@exit 1
endif
else
	@echo -e "\nError: you need to specify a usb drive, e.g., \"sdc\"."
	@echo -e "For example, run the following command:"
	@echo -e "   make boot usb=sdc\n"
	@echo -e "The following usb drives are currently attached to this system:"
	@lsblk -O | grep -i usb | awk '{print $$2}' | grep --color=never '[^0-9]$$'  # must escape $ in makefile with $$
	@echo ""
	@exit 1
endif


### Creates a bootable USB drive that can be inserted into a real PC based on the compiled .iso. 
boot: check_usb $(iso)
	@umount /dev/$(usb)* 2> /dev/null  |  true  # force it to return true
	@sudo dd bs=4M if=build/theseus-x86_64.iso of=/dev/$(usb)
	@sync
	

###################################################################################################
### This section contains targets to actually build Theseus components and create an iso file.
###################################################################################################

iso: $(iso)
grub-isofiles := build/grub-isofiles

### This target builds an .iso OS image from the userspace and kernel.
$(iso): kernel userspace $(grub_cfg)
	@rm -rf $(grub-isofiles) 
### copy userspace module build files
	@mkdir -p $(grub-isofiles)/modules
	@cp userspace/build/* $(grub-isofiles)/modules/
### copy kernel module build files and add the __k_ prefix
	@for f in kernel/build/* kernel/build/*/* ; do \
		cp -vf $${f}  $(grub-isofiles)/modules/`basename $${f} | sed -n -e 's/\(.*\)/__k_\1/p'` 2> /dev/null ; \
	done
### copy kernel boot image files
	@mkdir -p $(grub-isofiles)/boot/grub
	@cp $(nano_core) $(grub-isofiles)/boot/kernel.bin
	@cp $(grub_cfg) $(grub-isofiles)/boot/grub
	@grub-mkrescue -o $(iso) $(grub-isofiles)  2> /dev/null
	


### this builds all userspace programs
userspace: 
	@echo -e "\n======== BUILDING USERSPACE ========"
	@$(MAKE) -C userspace all


### this builds all kernel components
kernel: test_rustc
	@echo -e "\n======== BUILDING KERNEL ========"
	@$(MAKE) -C kernel all



clean:
	@rm -rf build
	@$(MAKE) -C kernel clean
	@$(MAKE) -C userspace clean
	


help: 
	@echo -e "\nThe following make targets are available:"
	@echo -e "  run:"
	@echo -e "\t The most common target. Builds and runs Theseus using the QEMU emulator."
	@echo -e "  debug:"
	@echo -e "\t Same as 'run', but pauses QEMU at its GDB stub entry point,"
	@echo -e "\t which waits for you to connect a GDB debugger using 'make gdb'."
	@echo -e "  gdb:"
	@echo -e "\t Runs a new instance of GDB that connects to an already-running QEMU instance."
	@echo -e "\t You must run 'make debug' beforehand in a separate terminal."
	@echo -e "  boot:"
	@echo -e "\t Builds Theseus as a bootable .iso and writes it to the specified USB drive."
	@echo -e "\t The USB drive is specified as usb=<dev-name>, e.g., 'make boot usb=sdc',"
	@echo -e "\t in which the USB drive is connected as /dev/sdc. This target requires sudo."
	@echo -e "\nThe following options are available for QEMU:"
	@echo -e "  int=yes:"
	@echo -e "\t Enable interrupt logging in QEMU console (-d int)."
	@echo -e "\t Only relevant for QEMU targets like 'run' and 'debug'."
	@echo ""
