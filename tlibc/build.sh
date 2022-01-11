#!/bin/bash
set -e
set -x

# capture all output to a file
# script -e .script_output

TLIBC_DIR="$(dirname "$(readlink -f "$0")")"
THESEUS_BASE_DIR=$TLIBC_DIR/..
THESEUS_CARGO_PATH="$THESEUS_BASE_DIR/tools/theseus_cargo"
THESEUS_DEPS_DIR="$THESEUS_BASE_DIR/build/deps"

## The file name of the compiled output of tlibc's C code components.
TLIBC_C_OUT_FILENAME="libtlibc_c.a"


export RUST_BACKTRACE=1

### Note: the "theseus_cargo" tool must be installed locally instead of invoked via `cargo run` 
cargo install --force --locked --path=$THESEUS_CARGO_PATH --root=$THESEUS_CARGO_PATH

### Do a full clean build every time at this point
cargo clean

### Use theseus_cargo to build this cargo package (tlibc) 
### with an automatic configuration that builds it to depend against pre-built Theseus crates.
$THESEUS_CARGO_PATH/bin/theseus_cargo  --input $THESEUS_DEPS_DIR  build

### Create a library archive (.a) file from all of the tlibc crate object files,
### being sure to include the C code components.
### Note: it's better to do a partial link, using `ld -r` below.
ar -rcs ./target/x86_64-theseus/release/libtlibc.a  \
    ./target/x86_64-theseus/release/deps/*.o        \
    $(find ./target/x86_64-theseus/release/build/tlibc* -name "*.o")

### Create a partially-linked object (.o) file from all of the tlibc crate object files,
### being sure to include the C code components.
ld -r -o  ./target/x86_64-theseus/release/tlibc.o   \
    ./target/x86_64-theseus/release/deps/*.o        \
    $(find ./target/x86_64-theseus/release/build/tlibc* -name "*.o")
    # $(find ./target/x86_64-theseus/release/ -name $TLIBC_C_OUT_FILENAME)


### Attempt to statically link everything together in a way we can overwrite the relocations later. 
### Note: this should be done only when building the final C application executable binary, not here.
#
# reset
# ld --emit-relocs -o  ./target/x86_64-theseus/release/tlibc_static  \
#     -u main  \
#     ./target/x86_64-theseus/release/deps/*.o  \
#     $THESEUS_BASE_DIR/target/x86_64-theseus/release/libnano_core.a \
#     $THESEUS_BASE_DIR/target/x86_64-theseus/release/deps/*.o \


    

