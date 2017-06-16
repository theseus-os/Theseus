SHELL := /bin/bash

RUSTC_CURRENT_SUPPORTED_VERSION := rustc 1.19.0-nightly (75b056812 2017-05-15)
RUSTC_OUTPUT=$(shell rustc --version)

KVM_CMD=$(shell kvm-ok 2>&1 > /dev/null; if [ $$? == 0 ]; then echo "-enable-kvm"; fi)

arch ?= x86_64
target ?= $(arch)-restful_os
kernel := build/kernel-$(arch).bin
iso := build/os-$(arch).iso

rust_os := target/$(target)/debug/librestful_os.a
linker_script := src/arch/arch_$(arch)/boot/linker.ld
grub_cfg := src/arch/arch_$(arch)/boot/grub.cfg
assembly_source_files := $(wildcard src/arch/arch_$(arch)/boot/*.asm)
assembly_object_files := $(patsubst src/arch/arch_$(arch)/boot/%.asm, \
	build/arch/$(arch)/%.o, $(assembly_source_files))

.PHONY: all clean run debug iso cargo gdb

test_rustc: 	
ifneq (${RUSTC_OUTPUT}, ${RUSTC_CURRENT_SUPPORTED_VERSION})
	# @echo '   Error: must use rustc version: "$(RUSTC_CURRENT_SUPPORTED_VERSION)"!!\n\n'
	$(error must use rustc version: "$(RUSTC_CURRENT_SUPPORTED_VERSION)")
	# @exit 1
endif


all: $(kernel)

clean:
	@cargo clean
	@rm -rf build

orun:
	@qemu-system-x86_64 -cdrom $(iso) -s  -serial stdio

odebug:
	@qemu-system-x86_64 -cdrom $(iso) -s -S -serial stdio

run: $(iso) 
	#@qemu-system-x86_64  -cdrom $(iso) -s  -serial stdio  -no-shutdown  -d int
	@qemu-system-x86_64 $(KVM_CMD) -cdrom $(iso) -s  -serial stdio  -no-shutdown -cpu Haswell

debug: $(iso)
	@qemu-system-x86_64 -cdrom $(iso) -s -S -serial stdio

gdb:
	@rust-os-gdb/bin/rust-gdb "build/kernel-x86_64.bin" -ex "target remote :1234"

iso: $(iso)

$(iso): $(kernel) $(grub_cfg)
	@mkdir -p build/isofiles/boot/grub
	@cp $(kernel) build/isofiles/boot/kernel.bin
	@cp $(grub_cfg) build/isofiles/boot/grub
	@grub-mkrescue -o $(iso) build/isofiles 2> /dev/null
	@rm -r build/isofiles

$(kernel): cargo $(rust_os) $(assembly_object_files) $(linker_script)
	@ld -n --gc-sections -T $(linker_script) -o $(kernel) $(assembly_object_files) $(rust_os)

cargo:  test_rustc
	@xargo build --target $(target)

# compile assembly files
build/arch/$(arch)/%.o: src/arch/arch_$(arch)/boot/%.asm
	@mkdir -p $(shell dirname $@)
	@nasm -felf64 $< -o $@
