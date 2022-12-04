.DEFAULT_GOAL := all
SHELL := /bin/bash

## Temporary hack to re-build `dummy` C targets every time
.PHONY: dummy dummy2 dummy_works dummy_pie

all: dummy_works

CROSS = x86_64-elf-

### Check that our custom cross compiler retargeted for Theseus exists and is on the current PATH.
ifeq (, $(shell which ${CROSS}gcc))
$(error "Missing ${CROSS}gcc in PATH, please add it to your path or install it using `scripts/install_x86_64-elf-gcc.sh`")
endif

### Disable the red zone because Theseus is a single privilege level OS
export override LDFLAGS += -mno-red-zone
### Don't waste memory Theseus doesn't use huge pages, so no need to waste memory
export override LDFLAGS += -z max-page-size=4096

### (Attempt) try to include libgcc as statically linked in. Not sure this does anything.
export override LDFLAGS += -static-libgcc

### NOTE: only the -pie argument actually matters, 
### but the gcc manual says to also include fpie/fPIE
PIE := -fpic -fPIC -fpie -fPIE -pie

TLIBC_DIR := ../tlibc
TLIBC_A := $(TLIBC_DIR)/target/x86_64-unknown-theseus/release/libtlibc.a
TLIBC_O := $(TLIBC_DIR)/target/x86_64-unknown-theseus/release/tlibc.o

GLIBC_DIR := $(HOME)/opt/cross/lib/gcc/x86_64-elf/10.2.0/no-red-zone
STARTFILES := $(GLIBC_DIR)/crtbegin.o
ENDFILES := $(GLIBC_DIR)/crtend.o

# LIBGROUP := -Wl,--start-group ../tlibc/target/x86_64-unknown-theseus/release/deps/*.o -lgcc -Wl,--end-group
LIBGROUP := -Wl,--start-group $(TLIBC_A) -lgcc -Wl,--end-group
# LIBGROUP := -Wl,--start-group $(TLIBC_O) -lgcc -Wl,--end-group
# LIBGROUP := -L$(TLIBC_DIR)/target/x86_64-unknown-theseus/release  -ltlibc

# PARTIAL_LINK := -Wl,--relocatable

# export override LDFLAGS += -nostdlib
export override LDFLAGS += -nostdlib -nostartfiles 
# export override LDFLAGS  = -nostdlib -nostartfiles -static


### Above options based on this link: <https://stackoverflow.com/a/10772056>


### This approach seems to be the closest to what I want.
### To use the outputted executable, we (Theseus's loader at runtime) just needs to go through all of the relocations
### that use OBJECT sections (.data, .rodata) as their source section and then re-write those entries to use the corresponding existing Theseus sections 
### as source sections instead.
###
### TODO: figure out a way to remove all irrelevant relocation entries, at a minimum the `R_X86_64_NONE` and local relocation sections, such as the `.rela.eh_frame` section. 
### 
%: %.c $(TLIBC_A)
	$(CROSS)gcc \
		$(LDFLAGS) \
		-O0 \
		-I $(TLIBC_DIR)/include \
		-static-libgcc \
		-ffunction-sections \
		-fdata-sections \
		-Wl,-gc-sections \
		-mcmodel=large \
		-Wl,--emit-relocs \
		-o $@ \
		$(PARTIAL_LINK) \
		$(STARTFILES) \
		$^ \
		$(TLIBC_A) \
		$(ENDFILES)

		# ../target/x86_64-unknown-theseus/release/libnano_core.a \

		# -static \
	
	# remove debug symbols, which are HUGE
	strip --strip-debug $@

# check this out: https://stackoverflow.com/questions/10465875/is-there-an-option-to-gnu-ld-to-omit-dynamic-linker-pt-interp-completely




### This approach is position independent (i think?), but currently doesn't use crtbegin.o and crtend.o (we need to recompile them with -fPIE).
### It also currently generates code that uses PLT and GOT, which I don't really want or need. 
### Perhaps we're using too many arguments in the $(PIE) variable? Or we could try `gcc -fno-plt -fno-pic`
dummy_pie:
	$(CROSS)gcc \
		$(LDFLAGS) \
		$(PIE) \
		-O0 \
		-static-libgcc \
		-ffunction-sections \
		-fdata-sections \
		-Wl,-gc-sections \
		-mcmodel=large \
		-Wl,--emit-relocs \
		-o $@ \
		$(PARTIAL_LINK) \
		dummy.c \
		$(TLIBC_A) \
		../target/x86_64-unknown-theseus/release/libnano_core.a


		# -static \
	
	# remove debug symbols, which are HUGE
	strip --strip-debug $@



### Below here are older attempts that didn't quite do what we want, 
### but I'm preserving them for potential use in the future. 

dummy: dummy.c
	@which $(CROSS)gcc
	@echo "PATH: $(PATH)"

# Note: `$@` is Make syntax for the target, i.e., "dummy"
#       `$^` is Make syntax for the dependencies, i.e., "dummy.c"

	$(CROSS)gcc \
		$(LDFLAGS) \
		$(PIE) \
		-O0 \
		-o $@ \
		$^ \
		start.c


dummy2: dummy.c
	@echo "LD_PRELOAD: $(LD_PRELOAD)"

	$(CROSS)gcc \
		$(LDFLAGS) \
		$(PIE) \
		-O0 \
		-static-libgcc \
		-o $@ \
		$(PARTIAL_LINK) \
		$(STARTFILES) \
		$^ \
		$(LIBGROUP) \
		$(ENDFILES)


	# $(CROSS)ld -r -o  dummy_ld_r_tlibc.o  $(TLIBC_O)  $@
		

		# start.c

		
		# -static \
