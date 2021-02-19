.DEFAULT_GOAL := all
SHELL := /bin/bash

all: build

### Use our custom cross compiler retargeted for Theseus1
export override PATH := $(HOME)/opt/cross/bin:$(PATH)

### Disable the red zone because Theseus is a single privilege level OS
export override LDFLAGS += -mno-red-zone
### Don't waste memory Theseus doesn't use huge pages, so no need to waste memory
export override LDFLAGS += -z max-page-size=4096

### NOTE: only the -pie argument actually matters, 
### but the gcc manual says to also include fpie/fPIE
# PIE := -fpic -fPIC -fpie -fPIE -pie

CROSS = x86_64-elf-

build: dummy.c
	which $(CROSS)gcc

	$(CROSS)gcc $(LDFLAGS) \
		-nostdlib \
		$(PIE) \
		-O0 \
		-o dummy \
		dummy.c \
		start_stub.S
	
		
		# -static \
