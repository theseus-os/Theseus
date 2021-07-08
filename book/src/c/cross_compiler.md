# Building GCC and Binutils to target Theseus (x86_64-elf)

**We provide a script that does all of this for you;** see [scripts/install_x86_64-elf-gcc.sh](https://github.com/theseus-os/Theseus/blob/theseus_main/scripts/install_x86_64-elf-gcc.sh), 
```sh
./scripts/install_x86_64-elf-gcc.sh $HOME/src $HOME/opt
```

---------------------------------------------------------------------------------

In this document, we refer to two directories, both of which can be set to the location of your choice:
 1. `$SRC`: the directory that contains the source for gcc and binutils
 	* For example, `$HOME/src/`
 2. `$DEST`: the directory that will hold our compiled gcc and binutils packages and libraries (where they will be installed)
 	* For example, `$HOME/opt/`

(Instructions were taken from [this tutorial on the OS dev wiki](https://wiki.osdev.org/Building_GCC).)

## 1. Build a standalone version of GCC & Binutils

Install the required packages:
```sh
sudo apt-get install gcc build-essential bison flex libgmp3-dev libmpc-dev libmpfr-dev texinfo gcc-multilib
```

### Download and build GCC/Binutils
For this tutorial, we'll use `gcc` version `10.2.0`, released July 20, 2020,
and `binutils` version `2.35.1`, released September 19, 2020.

You can obtain it from many mirrors online, such as these:
* gcc: <https://mirrors.kernel.org/gnu/gcc/gcc-10.2.0/>
* binutils: <https://mirrors.kernel.org/gnu/binutils/>

Create a destination directory for the newly-built packages to be installed into:
```sh
mkdir $DEST
export PREFIX="$DEST/gcc-10.2.0"
```

Extract each source code package into the directory of your choice (`$SRC`), and then build binutils:
```sh
mkdir build-binutils
cd build-binutils
../binutils-2.35.1/configure --prefix="$PREFIX" --disable-nls --disable-werror
make -j$(nproc)
make install
```

Then go back to the `$SRC` directory and build gcc:
```sh
# Use a GCC script to download all necessary prerequisites
cd gcc-10.2.0
./contrib/download_prerequisites
cd ../

mkdir build-gcc
cd build-gcc
../gcc-10.2.0/configure --prefix="$PREFIX" --disable-nls --enable-languages=c,c++
make -j$(nproc)
make install
```


## 2. Build GCC and Binutils again, to cross-target Theseus (x86_64-elf)
Now that we have a standalone build of gcc/binutils that is independent from the one installed by your host system's package manager, we can use that to build a version of gcc that inherently performs cross-compilation for a specific target, in this case, our Theseus `x86_64-elf` target.

Note: these instructions are based on [this tutorial from the OS dev wiki](https://wiki.osdev.org/GCC_Cross-Compiler#The_Build).

First, create a directory for the cross compiler to be built and installed into, e.g., `$DEST/cross`.
```sh
mkdir $DEST/cross
export PREFIX="$DEST/cross"
export TARGET=x86_64-elf
export PATH="$PREFIX/bin:$PATH"
```

Second, re-build the same binutils package as above, but in a way that configures it to target Theseus. 
```sh
../binutils-2.35.1/configure --target=$TARGET --prefix="$PREFIX" --with-sysroot --disable-nls --disable-werror
make -j$(nproc)
make install
```

Confirm that your new cross-compiler binutils package exists and is on your system PATH:
```sh
which --$TARGET-as 
```
should output something like:
> ```
> /home/my_username/opt/cross/bin/x86_64-elf-as
> ```

Then go back to the `$SRC` directory and build a version of gcc that cross compiles C/C++ programs to Theseus.     
```sh
mkdir cross-build-gcc
cd cross-build-gcc
../gcc-10.2.0/configure --target=$TARGET --prefix="$PREFIX" --disable-nls --enable-languages=c,c++ --without-headers
make all-gcc -j$(nproc)
make all-target-libgcc -j$(nproc) 
make install-gcc
make install-target-libgcc
```

Before moving on, let's check to make sure our cross-compiled gcc is working.
```sh
$DEST/cross/bin/$TARGET-gcc --version
```
This should print out some information about your newly-built gcc. Add the `-v` flag to dump out even more info. 


## 3. Re-building GCC without the default `red-zone` usage
Importantly, we must disable the [red zone](https://en.wikipedia.org/wiki/Red_zone_(computing)) in gcc entirely. When invoking gcc itself, we can simply pass the `-mno-red-zone` argument on the command line, but that doesn't affect the cross-compiled version of `libgcc` itself. Thus, in order to avoid `libgcc` functions invalidly using the non-existing red zone in Theseus, we have to build a no-red-zone version of `libgcc` in order to successfully build and link C programs for Theseus,  without `libgcc`'s methods trying to write to the red zone. 

Note: instructions were adapted from [this tutorial](https://wiki.osdev.org/Libgcc_without_red_zone).

### Adjusting the GCC config
First, create a new file within the gcc source tree at `$SRC/gcc-10.2.0/gcc/config/i386`.    
Add the following lines to that new file and save it:
```
MULTILIB_OPTIONS += mno-red-zone
MULTILIB_DIRNAMES += no-red-zone
```
Yes, even though we're building for `x86_64`, we put it in the original x86 architecture config folder called `i386`.

Then, instruct gcc's build process to use that new multilib configuration. Open the file `$SRC/gcc-10.2.0/gcc/config` and search for the following configuration lines, which starts on Line 1867 (for gcc-10.2.0):    
```
x86_64-*-elf*)
	tm_file="${tm_file} i386/unix.h i386/att.h dbxelf.h elfos.h newlib-stdint.h i386/i386elf.h i386/x86-64.h"
	;;
```
Add a line such that it looks like this:
```
x86_64-*-elf*)
	tmake_file="${tmake_file} i386/t-x86_64-elf"
	tm_file="${tm_file} i386/unix.h i386/att.h dbxelf.h elfos.h newlib-stdint.h i386/i386elf.h i386/x86-64.h"
	;;
```
**Note**: the indentation before `tmake_file` must be a TAB, not spaces. 

### Building GCC again with no red zone
Go back to the build directory and reconfigure and re-make libgcc:
```sh
cd $SRC/cross-build-gcc
../gcc-10.2.0/configure --target=$TARGET --prefix="$PREFIX" --disable-nls --enable-languages=c,c++ --without-headers
make all-gcc -j$(nproc)
make all-target-libgcc -j$(nproc) 
make install-gcc
make install-target-libgcc
```

To check that it worked, run the following two commands:
```sh
x86_64-elf-gcc -print-libgcc-file-name
x86_64-elf-gcc -mno-red-zone -print-libgcc-file-name
```

The first one should output a path to `libgcc.a`, and the second should output a similar path with `no-red-zone` as the containing directory:
> ```
> $DEST/cross/lib/gcc/x86_64-elf/10.2.0/libgcc.a
> $DEST/cross/lib/gcc/x86_64-elf/10.2.0/no-red-zone/libgcc.a
> ```

## Appendix: How to use the no-red-zone version of GCC
To properly use this new version of GCC that cross-compiles to the Theseus target and disables the red zone, make sure you:
 1. use the `x86_64-elf-gcc` executable that now resides in `$DEST/cross` 
 2. specify the `-mno-red-zone` flag, either on the command line or as part of `LDFLAGS`

<!-- cspell:ignore dbxelf, elfos, ldflags, libgcc, libgmp, libmpc, libmpfr, multilib, newlib, nproc, stdint, texinfo, tmake, werror -->
