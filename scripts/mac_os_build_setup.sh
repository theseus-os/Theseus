#!/bin/bash
set -e

### This script sets up the build environment of tools required for building Thesesus on macOS.
### Adapted from here: https://gist.github.com/emkay/a1214c753e8c975d95b4

#### Check if `brew` is installed
command -v brew >/dev/null 2>&1 || { echo >&2 "Missing homebrew (\`brew\`). Install it from http://brew.sh/."; exit 1; }

brew install make wget coreutils findutils nasm pkgconfig x86_64-elf-gcc x86_64-elf-binutils aarch64-elf-gcc aarch64-elf-binutils xorriso
### Install dependencies needed to cross-compile grub for macOS
brew install autoconf automake libtool pkg-config

src="/tmp/theseus_tools_src"
prefix="/usr/local"
target=x86_64-elf

mkdir -p "$src"

### Download and cross-compile objconv
cd "$src"
if ! command -v objconv &> /dev/null
then
  echo "Installing \`objconv\`"
  echo ""
  wget http://www.agner.org/optimize/objconv.zip -O objconv.zip
  mkdir -p build-objconv
  unzip -o objconv.zip -d build-objconv
  cd build-objconv
  unzip -o source.zip -d src
  g++ -o objconv -O2 src/*.cpp --prefix="$prefix"
  echo "Copying objconv binary to $prefix/bin requires sudo privileges"
  sudo cp objconv "$prefix/bin"
fi


### Download and cross-compile grub, which we need for the grub-mkrescue tool
cd "$src"
if ! command -v grub-mkrescue &> /dev/null
then
  echo "Installing \`grub\`"
  echo ""

  if [ ! -d "grub" ]
  then
    git clone --depth 1 https://github.com/theseus-os/grub.git
  fi

  cd grub

  PYTHON="python3" sh autogen.sh
  mkdir -p build-grub
  cd build-grub
  ../configure --disable-werror TARGET_CC=$target-gcc TARGET_OBJCOPY=$target-objcopy \
    TARGET_STRIP=$target-strip TARGET_NM=$target-nm TARGET_RANLIB=$target-ranlib \
  --target=$target --prefix="$prefix"
  make -j4
  echo "Installing grub tools to $prefix requires sudo privileges"
  sudo make install
fi

### Install qemu
brew install qemu 