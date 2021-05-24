#!/bin/bash
set -e

### This script sets up the build environment of tools required for building Thesesus on macOS.
### Adapted from here: https://gist.github.com/emkay/a1214c753e8c975d95b4


#### Check if `brew` is installed
command -v brew >/dev/null 2>&1 || { echo >&2 "Missing homebrew (\`brew\`). Install it from http://brew.sh/."; exit 1; }
# brew install wget gettext git pkg-config gmp mpfr libmpc autoconf automake nasm xorriso mtools qemu
#brew link --force gettext
#### force re-fetch our own gcc cross compilers tap recipe
#brew untap theseus-os/gcc_cross_compilers || true
#brew tap theseus-os/gcc_cross_compilers
#brew install x86_64-elf-binutils x86_64-elf-gcc


### Using mac ports instead of our own x86_64-elf-gcc homebrew tap recipe
which port || { echo >&2 "Missing installation of MacPorts (\`port\`)."; exit 1; }
sudo port install gmake wget coreutils findutils nasm pkgconfig x86_64-elf-gcc
sudo ln -s /opt/local/bin/gmake /opt/local/bin/make
### Macports doesn't have xorriso
brew install xorriso
### Install dependencies needed to cross-compile grub for macOS
brew install autoconf automake libtool pkg-config


export SRC_DIR="$HOME/theseus_tools_src"
export PREFIX="$HOME/theseus_tools_opt/"
export TARGET=x86_64-elf
export PATH="$PREFIX/bin:$PATH"

mkdir -p "$SRC_DIR"
mkdir -p "$PREFIX/bin/"



### Download and cross-compile objconv
cd "$SRC_DIR"
if [ ! -d "objconv" ]; then
  echo "Installing \`objconv\`"
  echo ""
  wget http://www.agner.org/optimize/objconv.zip
  mkdir -p build-objconv
  unzip objconv.zip -d build-objconv
  cd build-objconv
  unzip source.zip -d src
  g++ -o objconv -O2 src/*.cpp --prefix="$PREFIX"
  cp objconv "$PREFIX/bin/"
fi


### Download and cross-compile grub, which we need for the grub-mkrescue tool
cd "$SRC_DIR"
if [ ! -d "grub" ]; then
  echo "Installing \`grub\`"
  echo ""
  git clone --depth 1 https://github.com/theseus-os/grub.git
  cd grub
  sh autogen.sh
  mkdir -p build-grub
  cd build-grub
  ../configure --disable-werror TARGET_CC=$TARGET-gcc TARGET_OBJCOPY=$TARGET-objcopy \
    TARGET_STRIP=$TARGET-strip TARGET_NM=$TARGET-nm TARGET_RANLIB=$TARGET-ranlib --target=$TARGET --prefix="$PREFIX"
  make -j4
  make install
fi

### install qemu using Homebrew 
brew install qemu 