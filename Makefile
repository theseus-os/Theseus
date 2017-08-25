SHELL := /bin/bash

RUSTC_CURRENT_SUPPORTED_VERSION := rustc 1.21.0-nightly (c417ee9ae 2017-07-25)
RUSTC_CURRENT_INSTALL_VERSION := nightly-2017-07-26
RUSTC_OUTPUT=$(shell rustc --version)


#### We're disabling KVM for the time being because it breaks some features, like RDMSR used for TSC
KVM_CMD=
#KVM_CMD=$(shell kvm-ok 2>&1 > /dev/null; if [ $$? == 0 ]; then echo "-enable-kvm"; fi)


arch ?= x86_64
target ?= $(arch)-restful_os
kernel := build/kernel-$(arch).bin
iso := build/os-$(arch).iso

rust_os := target/$(target)/debug/librestful_os.a
linker_script := src/arch/arch_$(arch)/boot/linker_higher_half.ld
grub_cfg := src/arch/arch_$(arch)/boot/grub.cfg
assembly_source_files := $(wildcard src/arch/arch_$(arch)/boot/*.asm)
assembly_object_files := $(patsubst src/arch/arch_$(arch)/boot/%.asm, \
	build/arch/$(arch)/%.o, $(assembly_source_files))


# from quantum OS / Tifflin's baremetal rust-os kernel
LINKFLAGS := -T $(linker_script)
LINKFLAGS += -Map build/map.txt  # optional
LINKFLAGS += --gc-sections
LINKFLAGS += -z max-page-size=4096
LINKFLAGS += -n ## from phil's blog_os

CROSSDIR ?= prebuilt/x86_64-elf/bin

QEMU_MEMORY ?= 1G
QEMU_FLAGS := -cdrom $(iso) -no-reboot -no-shutdown -s -m $(QEMU_MEMORY) -serial stdio -cpu Haswell -net none

#drive and devices commands from http://forum.osdev.org/viewtopic.php?f=1&t=26483 to use sata emulation
QEMU_FLAGS += -drive format=raw,file=random_data2.img,if=none,id=mydisk -device ide-hd,drive=mydisk,bus=ide.0,serial=4696886396 

ifeq ($(int),yes)
	QEMU_FLAGS += -d int
endif
ifeq ($(kvm),yes)
	QEMU_FLAGS += -enable-kvm
endif



.PHONY: all clean run debug iso userspace cargo gdb

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
	@echo '\nFound proper rust compiler version, proceeding with build...\n'
endif


all: $(kernel)

clean:
	@cargo clean
	@rm -rf build

orun:
	@qemu-system-x86_64 $(QEMU_FLAGS)

odebug:
	@qemu-system-x86_64 $(QEMU_FLAGS) -S

run: $(iso) 
	@qemu-img resize random_data2.img 100K
	qemu-system-x86_64 $(QEMU_FLAGS)

	

debug: $(iso)
	@qemu-system-x86_64 $(QEMU_FLAGS) -S
#-monitor stdio

gdb:
	@rust-os-gdb/bin/rust-gdb "build/kernel-x86_64.bin" -ex "target remote :1234"

iso: $(iso)

$(iso): $(kernel) userspace $(grub_cfg)
	@rm -rf build/isofiles 
### copy userspace build files
	@mkdir -p build/isofiles/modules
	@cp userspace/build/* build/isofiles/modules/
### copy kernel build files
	@mkdir -p build/isofiles/boot/grub
	@cp $(kernel) build/isofiles/boot/kernel.bin
	@cp $(grub_cfg) build/isofiles/boot/grub
	@grub-mkrescue -o $(iso) build/isofiles  # 2> /dev/null
	


### this builds all userspace programs
userspace: 
	@echo "======== BUILDING USERSPACE ========"
	@$(MAKE) -C userspace all


$(kernel): cargo $(rust_os) $(assembly_object_files) $(linker_script)
	@ld -n --gc-sections -T $(linker_script) -o $(kernel) $(assembly_object_files) $(rust_os)	


cargo:  test_rustc
	@xargo build --target $(target)


# compile assembly files
build/arch/$(arch)/%.o: src/arch/arch_$(arch)/boot/%.asm
	@mkdir -p $(shell dirname $@)
	@nasm -felf64 $< -o $@
