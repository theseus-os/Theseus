### This Makefile builds only the nano-core object files (as a static library ".a" file), 
### consisting of the nano-core's Rust crate modules and its assembly boot files.
### It does not build an OS .iso image or anything runnable in a simulator like QEMU.

.DEFAULT_GOAL := all


arch ?= x86_64

## this should not point directly to the .json target spec file, but it should have the same name.
target ?= $(arch)-theseus

## this is the output path of the xargo command, not our choice.
## The name of the lib (lib_____.a) is defined by the target above.
cargo-obj := target/$(target)/debug/libtheseus.a

nano-core-obj := build/nano-core-$(arch).bin

linker_script := src/arch/arch_$(arch)/boot/linker_higher_half.ld
assembly_source_files := $(wildcard src/arch/arch_$(arch)/boot/*.asm)
assembly_object_files := $(patsubst src/arch/arch_$(arch)/boot/%.asm, \
	build/arch/$(arch)/%.o, $(assembly_source_files))


.PHONY: all clean cargo


all: $(nano-core-obj)

clean:
	@cargo clean
	@rm -rf build


cargo: 
	@xargo build --target $(target)  ## target arg must be a target name, not the file path.


$(nano-core-obj): cargo $(cargo-obj) $(assembly_object_files) $(linker_script)
	@ld -n --gc-sections -T $(linker_script) -o $(nano-core-obj) $(assembly_object_files) $(cargo-obj)	


# compile assembly files
build/arch/$(arch)/%.o: src/arch/arch_$(arch)/boot/%.asm
	@mkdir -p $(shell dirname $@)
	@nasm -felf64 $< -o $@
