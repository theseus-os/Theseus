SHELL := /bin/bash

RUSTC_CURRENT_SUPPORTED_VERSION := rustc 1.19.0-nightly
RUSTC_OUTPUT=$(shell rustc --version)


#### We're disabling KVM for the time being because it breaks some features, like RDMSR
KVM_CMD=
#KVM_CMD=$(shell kvm-ok 2>&1 > /dev/null; if [ $$? == 0 ]; then echo "-enable-kvm"; fi)


arch ?= x86_64
target ?= $(arch)-restful_os
kernel := build/kernel-$(arch).bin
iso := build/os-$(arch).iso

rust_os := target/$(target)/debug/librestful_os.a
linker_script := src/arch/arch_$(arch)/boot/linker_higher_half.ld
grub_cfg := src/arch/arch_$(arch)/boot/grub.cfg
assembly_source_files := $(wildcard src/arch/arch_$(arch)/boot/*.S)
assembly_object_files := $(patsubst src/arch/arch_$(arch)/boot/%.S, \
	build/arch/$(arch)/%.o, $(assembly_source_files))


# from quantum OS / Tifflin's baremetal rust-os kernel
LINKFLAGS := -T $(linker_script)
LINKFLAGS += -Map build/map.txt  # optional
LINKFLAGS += --gc-sections
LINKFLAGS += -z max-page-size=4096
LINKFLAGS += -n ## from phil's blog_os

CROSSDIR ?= prebuilt/x86_64-elf/bin

QEMU_MEMORY ?= -m 10G

.PHONY: all clean run debug iso userspace cargo gdb

test_rustc: 	
ifneq (, $(findstring ${RUSTC_CURRENT_SUPPORTED_VERSION}, ${RUSTC_OUTPUT}))
	@echo '   Found proper rust compiler version, proceeding with build...'
else
	# @echo '   Error: must use rustc version: "$(RUSTC_CURRENT_SUPPORTED_VERSION)"!!\n\n'
	$(error must use rustc version: "$(RUSTC_CURRENT_SUPPORTED_VERSION)")
	# @exit 1
endif


all: $(kernel)

clean:
	@cargo clean
	@rm -rf build

orun:
	@qemu-system-x86_64 $(KVM_CMD) $(QEMU_MEMORY) -cdrom $(iso) -s  -serial stdio

odebug:
	@qemu-system-x86_64 $(QEMU_MEMORY) -cdrom $(iso) -s -S -serial stdio

run: $(iso) 
	@qemu-system-x86_64 $(KVM_CMD) $(QEMU_MEMORY) -cdrom $(iso) -s  -serial stdio  -no-reboot -no-shutdown

debug: $(iso)
	@qemu-system-x86_64 $(QEMU_MEMORY) -cdrom $(iso) -s -S -serial stdio -d int  -no-reboot -no-shutdown 
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
	@$(MAKE) -C userspace



$(kernel): cargo $(rust_os) $(assembly_object_files) $(linker_script)
	@$(CROSSDIR)/x86_64-elf-ld $(LINKFLAGS) -o $(kernel) $(assembly_object_files) $(rust_os)

cargo:  test_rustc
	@xargo build --target $(target)


### to build x86_64-elf-*, follow this: http://os.phil-opp.com/cross-compile-binutils/
build/arch/$(arch)/%.o: src/arch/arch_$(arch)/boot/boot.S
	@mkdir -p $(shell dirname $@)
	@$(CROSSDIR)/x86_64-elf-as -o $@ $<
