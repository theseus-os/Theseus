### This makefile is the top-level build script that builds all the crates in subdirectories 
### and combines them into the final OS .iso image.
### It also provides convenient targets for running and debugging Theseus and using GDB on your host computer.
.DEFAULT_GOAL := all
SHELL := /bin/bash

.PHONY: all check_rustc check_xargo clean run debug iso kernel applications userspace cargo gdb doc docs view-doc view-docs


ARCH ?= x86_64
TARGET ?= $(ARCH)-theseus
nano_core := kernel/build/nano_core-$(ARCH).bin
iso := build/theseus-$(ARCH).iso

ifeq ($(bypass),yes)
	BYPASS_RUSTC_CHECK := yes
endif


all: iso


###################################################################################################
### For ensuring that the host computer has the proper version of the Rust compiler
###################################################################################################

RUSTC_CURRENT_SUPPORTED_VERSION := rustc 1.27.0-nightly (ac3c2288f 2018-04-18)
RUSTC_CURRENT_INSTALL_VERSION := nightly-2018-04-19
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

XARGO_CURRENT_SUPPORTED_VERSION := 0.3.10
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
### This section has QEMU arguments and configuration
###################################################################################################
QEMU_MEMORY ?= 512M
QEMU_FLAGS := -cdrom $(iso) -no-reboot -no-shutdown -s -m $(QEMU_MEMORY) -serial stdio 
## the most recent CPU model supported by QEMU 2.5.0
QEMU_FLAGS += -cpu host
## multicore 
QEMU_FLAGS += -smp 4

## basic networking with a standard e1000 ethernet card
#QEMU_FLAGS += -net nic,vlan=0,model=e1000,macaddr=00:0b:82:01:fc:42 -net dump,file=netdump.pcap
QEMU_FLAGS += -net nic,vlan=1,model=e1000,macaddr=00:0b:82:01:fc:42 -net user,vlan=1 -net dump,file=netdump.pcap
#QEMU_FLAGS += -net nic,vlan=1,model=e1000 -net user,vlan=1 -net dump,file=netdump.pcap

## drive and devices commands from http://forum.osdev.org/viewtopic.php?f=1&t=26483 to use sata emulation
QEMU_FLAGS += -drive format=raw,file=random_data2.img,if=none,id=mydisk -device ide-hd,drive=mydisk,bus=ide.0,serial=4696886396 -enable-kvm

ifeq ($(int),yes)
	QEMU_FLAGS += -d int
endif
ifeq ($(kvm),yes)
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



### Currently, loadable module mode requires release build mode
# loadable : export RUST_FEATURES = --package nano_core --features loadable
loadable : export RUST_FEATURES = --manifest-path "nano_core/Cargo.toml" --features loadable
loadable : export BUILD_MODE = release
loadable: run


### builds and runs Theseus in QEMU
run: $(iso) 
	@qemu-img resize random_data2.img 100K
	qemu-system-x86_64 $(QEMU_FLAGS)


### builds and runs Theseus in QEMU, but pauses execution until a GDB instance is connected.
debug: $(iso)
	@qemu-system-x86_64 $(QEMU_FLAGS) -S
#-monitor stdio


### Runs a gdb instance on the host machine. 
### Run this after invoking "make debug" in a different terminal.
gdb:
	@rust-os-gdb/bin/rust-gdb "$(nano_core)" -ex "target remote :1234"



### builds and runs Theseus in Bochs
# bochs : export RUST_FEATURES = --package apic --features apic_timer_fixed
bochs : export RUST_FEATURES = --manifest-path "apic/Cargo.toml" --features "apic_timer_fixed"
bochs: $(iso) 
	#@qemu-img resize random_data2.img 100K
	bochs -f bochsrc.txt -q



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
# boot : export RUST_FEATURES = --package captain --features mirror_serial
boot : export RUST_FEATURES = --manifest-path "captain/Cargo.toml" --features "mirror_serial"
boot: check_usb $(iso)
	@umount /dev/$(usb)* 2> /dev/null  |  true  # force it to return true
	@sudo dd bs=4M if=build/theseus-x86_64.iso of=/dev/$(usb)
	@sync
	

###################################################################################################
### This section contains targets to actually build Theseus components and create an iso file.
###################################################################################################

iso: $(iso)
grub-isofiles := build/grub-isofiles


### This target builds an .iso OS image from the applications and kernel.
### It skips userspace for now, but you can add it back in easily on the line below.
$(iso): kernel applications
# after building kernel and application modules, copy the kernel boot image files
	@mkdir -p $(grub-isofiles)/boot/grub
	@cp $(nano_core) $(grub-isofiles)/boot/kernel.bin
# autogenerate the grub.cfg file
	cargo run --manifest-path tools/grub_cfg_generation/Cargo.toml -- $(grub-isofiles)/modules/ -o $(grub-isofiles)/boot/grub/grub.cfg
	@grub-mkrescue -o $(iso) $(grub-isofiles)  2> /dev/null


# ### This target builds an .iso OS image from the userspace and kernel.
# $(iso): kernel userspace
# # after building kernel and userspace modules, copy the kernel boot image files
# 	@mkdir -p $(grub-isofiles)/boot/grub
# 	@cp $(nano_core) $(grub-isofiles)/boot/kernel.bin
# # autogenerate the grub.cfg file
#	cargo run --manifest-path tools/grub_cfg_generation/Cargo.toml -- $(grub-isolfiles)/modules/ -o $(grub-isofiles)/boot/grub/grub.cfg
# 	@grub-mkrescue -o $(iso) $(grub-isofiles)  2> /dev/null
	


### this builds all applications, which run in the kernel
applications: check_rustc check_xargo
	@echo -e "\n======== BUILDING APPLICATIONS ========"
	@$(MAKE) -C applications all
# copy applications' object files
	@mkdir -p $(grub-isofiles)/modules
	@for f in  ./applications/build/*.o ; do \
		cp -vf  $${f}  $(grub-isofiles)/modules/  ; \
	done
### TODO FIXME: not sure if it's correct to forcibly overwrite all module files with the applications' version (the cp line above),
### since sometimes the kernel is build in debug mode but applications are alwasy built in release mode right now 


### this builds all userspace programs
userspace: 
	@echo -e "\n======== BUILDING USERSPACE ========"
	@$(MAKE) -C userspace all
# copy userspace binary files and add the __u_ prefix
	@mkdir -p $(grub-isofiles)/modules
	@for f in `find ./userspace/build -type f` ; do \
		cp -vf $${f}  $(grub-isofiles)/modules/`basename $${f} | sed -n -e 's/\(.*\)/__u_\1/p'` 2> /dev/null ; \
	done


### this builds all kernel components
kernel: check_rustc check_xargo
	@echo -e "\n======== BUILDING KERNEL ========"
	@$(MAKE) -C kernel all
# copy kernel module build files
	@mkdir -p $(grub-isofiles)/modules
	@for f in `find ./kernel/build -maxdepth 1 -type f` ; do \
		cp -vf  $${f}  $(grub-isofiles)/modules/  ; \
	done
# copy the core library's object file
	@cp -vf $(HOME)/.xargo/lib/rustlib/$(TARGET)/lib/core-*.o $(grub-isofiles)/modules/__k_core.o


DOC_ROOT := build/doc/Theseus/index.html

doc:
	@rm -rf build/doc
	@mkdir -p build
	@$(MAKE) -C kernel doc
	@cp -rf kernel/target/doc ./build/
	@echo -e "\n\nDocumentation is now available in the build/doc directory."
	@echo -e "You run 'make view-doc' to view it, or just open $(DOC_ROOT)"

docs: doc


## Opens the documentation root in the system's default browser. 
## the "powershell" command is used on Windows Subsystem for Linux
view-doc: doc
	@xdg-open $(DOC_ROOT) > /dev/null 2>&1 || powershell.exe -c $(DOC_ROOT) &

view-docs: view-doc


clean:
	@rm -rf build
	@$(MAKE) -C kernel clean
	@$(MAKE) -C applications clean
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
	@echo -e "  bochs:"
	@echo -e "\t Same as 'make run', but runs Theseus in the Bochs emulator instead of QEMU."
	@echo -e "  boot:"
	@echo -e "\t Builds Theseus as a bootable .iso and writes it to the specified USB drive."
	@echo -e "\t The USB drive is specified as usb=<dev-name>, e.g., 'make boot usb=sdc',"
	@echo -e "\t in which the USB drive is connected as /dev/sdc. This target requires sudo."
	@echo -e "  doc:"
	@echo -e "\t Builds Theseus documentation from its Rust source code (rustdoc)."
	@echo -e "  view-doc:"
	@echo -e "\t Builds Theseus documentation and then opens it in your default browser."
	@echo -e "\nThe following options are available for QEMU:"
	@echo -e "  int=yes:"
	@echo -e "\t Enable interrupt logging in QEMU console (-d int)."
	@echo -e "\t Only relevant for QEMU targets like 'run' and 'debug'."
	@echo ""
