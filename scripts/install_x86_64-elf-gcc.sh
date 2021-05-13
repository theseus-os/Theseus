#!/bin/bash
set -e

### This script builds and installs GCC and binutils that are cross-compiled to target Theseus on x86_64. 
### It has been tested on Ubuntu 20.04 but will likely work on any Debian-based distribution.
### Note: this script takes about 45-60 minutes to execute, as it builds gcc twice. Go grab a drink!
###
### This script should be invoked with two arguments: 
### 1.  a "source" directory, where the GCC/binutils source files will be downloaded and built 
### 2.  a "destination" directory, where the GCC/binutils executables and libraries will be installed.
###
### For example, you can invoke it as such:  `sh scripts/install_x86_64-elf-gcc.sh ~/src ~/opt `

### Check that two arguments were provided.
if [ $# -lt 2 ] ; then
    echo "Error: expected two arguments: (1) a source directory, and (2) a destination directory."
    exit 1
fi

SRC="$(readlink -m $1)"
DEST="$(readlink -m $2)"

mkdir -p $SRC
mkdir -p $DEST

[ ! -d $SRC ]  && echo "Source directory '$SRC' is invalid."       && exit 1
[ ! -d $DEST ] && echo "Destination directory '$DEST' is invalid." && exit 1

echo "--> Downloading sources to '$SRC', installing gcc and binutils to '$DEST' ..."


### Install dependencies
sudo apt-get install -y gcc build-essential bison flex libgmp3-dev libmpc-dev libmpfr-dev texinfo gcc-multilib


### Obtain source files
cd $SRC
wget https://mirrors.kernel.org/gnu/gcc/gcc-10.2.0/gcc-10.2.0.tar.gz
wget https://mirrors.kernel.org/gnu/binutils/binutils-2.35.1.tar.gz
tar xzf gcc-10.2.0.tar.gz
tar xzf binutils-2.35.1.tar.gz

### Build binutils
export PREFIX="$DEST/gcc-10.2.0"
mkdir -p build-binutils
cd build-binutils
../binutils-2.35.1/configure --prefix="$PREFIX" --disable-nls --disable-werror
make -j$(nproc)
make install

### Build gcc, using a gcc-included script to download its dependencies
cd $SRC/gcc-10.2.0
./contrib/download_prerequisites
cd $SRC
mkdir -p build-gcc
cd build-gcc
../gcc-10.2.0/configure --prefix="$PREFIX" --disable-nls --enable-languages=c,c++
make -j$(nproc)
make install

### Now we have a standalone build of gcc and binutils,
### so let's use it to build a new version of gcc that cross-compiles for the Theseus target.
mkdir -p $DEST/cross
export PREFIX="$DEST/cross"
export TARGET=x86_64-elf
export PATH="$PREFIX/bin:$PATH"

### Build a cross-compiled version of binutils that targets Theseus.
cd $SRC
mkdir -p cross-build-binutils
cd cross-build-binutils
../binutils-2.35.1/configure --target=$TARGET --prefix="$PREFIX" --with-sysroot --disable-nls --disable-werror
make -j$(nproc)
make install

### Configure our new build of gcc to not use the red-zone for the x86_64-elf target.
### This involves creating a new file with the multilib options, and then telling gcc to use that new config file.
printf "MULTILIB_OPTIONS += mno-red-zone\nMULTILIB_DIRNAMES += no-red-zone" > $SRC/gcc-10.2.0/gcc/config/i386/t-x86_64-elf
### Note: in the following `sed` command, there is an escaped '\<TAB>' character right before 'tmake_file', that must be there.
###       Also, we use "1867 a" to indicate that we want to append a new line after Line 1867.
sed -i '1867 a \	tmake_file=\"\${tmake_file} i386/t-x86_64-elf\"' $SRC/gcc-10.2.0/gcc/config.gcc

### Build a cross-compiler version of gcc for Theseus on x86_64 without red-zone usage.
cd $SRC
mkdir -p $SRC/cross-build-gcc
cd cross-build-gcc
../gcc-10.2.0/configure --target=$TARGET --prefix="$PREFIX" --disable-nls --enable-languages=c,c++ --without-headers
make all-gcc -j$(nproc)
make all-target-libgcc -j$(nproc) 
make install-gcc
make install-target-libgcc


echo "\033[1;32m\n\nThe build of $TARGET gcc and binutils has completed.\033[0m \nShowing relevant libgcc files below:"
$TARGET-gcc -print-libgcc-file-name
$TARGET-gcc -mno-red-zone -print-libgcc-file-name

echo "\n[Optional] to clean up the build files, you can run the following:
	rm -rf $SRC/binutils-2.35.1
	rm -rf $SRC/binutils-2.35.1.tar.gz
	rm -rf $SRC/build-binutils
	rm -rf $SRC/build-gcc
	rm -rf $SRC/cross-build-binutils
	rm -rf $SRC/cross-build-gcc
	rm -rf $SRC/gcc-10.2.0
	rm -rf $SRC/gcc-10.2.0.tar.gz
	"

echo "\n\033[1;33mYou must add the new gcc binaries to your path by running the following command:
(to make it permanent, add it to the end of your .bashrc or your shell's .profile file)\033[0m
	export PATH=\"$DEST/cross/bin:\$PATH\"
	"
