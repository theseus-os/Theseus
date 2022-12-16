# Theseus Extra Files
This directory contains extra files that are copied directly into the ISO image after Theseus is built.


## Usage Examples
This can be a catch-all space for any random files that aren't necessarily source code or build artifacts, but still need to be present in Theseus's initial filesystem. 
Examples include:
* test text files
* WASM binaries
* Images
* Other resources


## How it works
All files and directories here will be copied as-is without modification into the `/extra_files/` directory within Theseus.
Directory hierarchies are preserved as well, but empty directories are ignored. Hierarchies are encoded using an exclamation
mark when creating the bootloader modules.

Here are some examples of how file paths would work:
| Host machine     | Bootloader module          | Runtime path                |
|------------------|----------------------------|-----------------------------|
| ./hello.txt      | extra_files!hello.txt      | /extra_files/hello.txt      |
| ./wasm/test.wasm | extra_files!wasm!test.wasm | /extra_files/wasm/test.wasm |
| ./foo/bar/me.o   | extra_files!foo!bar!me.o   | /extra_files/foo/bar/me.o   |
