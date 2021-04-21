#!/bin/bash
set -e

# capture all output to a file
# script -e .script_output

THIS_DIR="$(dirname "$(readlink -f "$0")")"
THESEUS_BASE_DIR=$THIS_DIR/..
THESEUS_CARGO_PATH="$THESEUS_BASE_DIR/tools/theseus_cargo"

export RUST_BACKTRACE=1

### Note: the "theseus_cargo" tool must be installed locally instead of invoked via `cargo run` 
cargo install --force --path=$THESEUS_CARGO_PATH --root=$THESEUS_CARGO_PATH

### Do a full clean build every time at this point
cargo clean

### Use theseus_cargo to build this cargo package (tlibc) 
### with an automatic configuration that builds it to depend against pre-built Theseus crates.
$THESEUS_CARGO_PATH/bin/theseus_cargo --input ../build/deps build

### Create a static library archive (.a file) from all of the tlibc crate object files.
ar -rcs ./target/x86_64-theseus/release/libtlibc.a ./target/x86_64-theseus/release/deps/*.o

ld -r -o  ./target/x86_64-theseus/release/tlibc.o ./target/x86_64-theseus/release/deps/*.o

