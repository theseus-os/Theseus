.DEFAULT_GOAL := all
SHELL := /bin/bash

all: dummy

### Use our custom cross compiler retargeted for Theseus1
export override PATH := $(HOME)/opt/cross/bin:$(PATH)

### Disable the red zone because Theseus is a single privilege level OS
export override LDFLAGS += -mno-red-zone
### Don't waste memory Theseus doesn't use huge pages, so no need to waste memory
export override LDFLAGS += -z max-page-size=4096

### NOTE: only the -pie argument actually matters, 
### but the gcc manual says to also include fpie/fPIE
# PIE := -fpic -fPIC -fpie -fPIE -pie

TLIBC_DIR := ../tlibc/target/x86_64-theseus/release
TLIBC := $(TLIBC_DIR)/tlibc-13d19183eba65090.o

CROSS = x86_64-elf-

LDFLAGS += -nostdlib -nostartfiles 
# LDFLAGS  = -nostdlib -nostartfiles -static


### Above options based on this link: <https://stackoverflow.com/a/10772056>


dummy: dummy.c
	which $(CROSS)gcc

	# Note: `$@` is Make syntax for the target, i.e., "dummy"
	#       `$^` is Make syntax for the dependencies, i.e., "dummy.c"

	$(CROSS)gcc $(LDFLAGS) \
		$(PIE) \
		-O0 \
		$(LDFLAGS) \
		-o $@ \
		$^ \
		-lgcc -lc -lgcc -lc


		# start.c

		
		# -static \
