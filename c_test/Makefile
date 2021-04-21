.DEFAULT_GOAL := all
SHELL := /bin/bash

## Temporary hack to re-build `dummy` every time
.PHONY: dummy

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

TLIBC_DIR := ../tlibc
TLIBC := $(TLIBC_DIR)/target/x86_64-theseus/release/libtlibc.a

CROSS = x86_64-elf-
GLIBC_DIR := $(HOME)/opt/cross/lib/gcc/x86_64-elf/10.2.0/no-red-zone
STARTFILES := $(GLIBC_DIR)/crtbegin.o
ENDFILES := $(GLIBC_DIR)/crtend.o

# LIBGROUP := -Wl,--start-group $(TLIBC) -lgcc -Wl,--end-group
# LIBGROUP := -Wl,--start-group ../tlibc/target/x86_64-theseus/release/deps/*.o -lgcc -Wl,--end-group
LIBGROUP := -L$(TLIBC_DIR)/target/x86_64-theseus/release  -ltlibc

PARTIAL_LINK := -Wl,--relocatable

# export override LDFLAGS += -nostdlib
export override LDFLAGS += -nostdlib -nostartfiles 
# export override LDFLAGS  = -nostdlib -nostartfiles -static


### Above options based on this link: <https://stackoverflow.com/a/10772056>


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
		-o $@ \
		$(PARTIAL_LINK) \
		$(STARTFILES) \
		$^ \
		$(LIBGROUP) \
		$(ENDFILES)
		
		#-lgcc -lc -lgcc -lc


		# start.c

		
		# -static \



# Missing non-rust symbols:
# malloc
# free
# abort
# strlen
# _ITM_registerTMCloneTable
# _ITM_deregisterTMCloneTable
# _GLOBAL_OFFSET_TABLE_   # should be removed when moving to large code model
# 