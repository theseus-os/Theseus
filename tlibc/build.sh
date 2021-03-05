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


### This is our new auto-config'd cargo command
$THESEUS_CARGO_PATH/bin/theseus_cargo --input ../build/deps build



## The newer NEWER raw cargo command that works without xargo at all, using cargo build-std.
## We don't want to pass the build-std flags to cargo since that would re-build all of the core files from scratch. 
## Instead, we want to re-use the existing pre-built core files from our previously-created sysroot output directory.
# RUST_TARGET_PATH="$THESEUS_BASE_DIR/build/deps"   \
# 	RUSTFLAGS="--emit=obj -C debuginfo=2 -C code-model=large -C relocation-model=static -D unused-must-use -Z merge-functions=disabled -Z share-generics=no --sysroot $THESEUS_BASE_DIR/build/deps/sysroot" \
# 	cargo build --release  --verbose \
# 	--target x86_64-theseus


## The "newer" raw cargo command that works without xargo at all. This is good because we can use a pre-built/distributed sysroot directory.
# RUST_TARGET_PATH="$THESEUS_BASE_DIR/build/deps"   \
# 	RUSTFLAGS="--emit=obj -C debuginfo=2 -C code-model=large -C relocation-model=static -D unused-must-use -Z merge-functions=disabled -Z share-generics=no --sysroot $THESEUS_BASE_DIR/build/deps/sysroot"  \
# 	cargo build --release --verbose -vv \
# 	--target x86_64-theseus


## The initial normal command that uses `xargo`
# RUST_TARGET_PATH="$THESEUS_BASE_DIR/cfg"  \
# 	RUSTFLAGS="--emit=obj -C debuginfo=2 -C code-model=large -C relocation-model=static -D unused-must-use -Z merge-functions=disabled -Z share-generics=no" \
# 	xargo build  --release  --verbose -vv \
# 	--target x86_64-theseus	



# RUST_TARGET_PATH="$THESEUS_BASE_DIR/cfg" \
# 	rustc --crate-name libtheseus src/lib.rs  --crate-type lib \
# 	--emit=dep-info,metadata,link \
# 	-C opt-level=3 -C embed-bitcode=no -C codegen-units=1 -C metadata=43462c60d48a531a -C extra-filename=-43462c60d48a531a \
# 	--out-dir $THESEUS_BASE_DIR/libtheseus/target/x86_64-theseus/release/deps \
# 	--target x86_64-theseus \
# 	-L dependency=$THESEUS_BASE_DIR/target/x86_64-theseus/release/deps \
# 	--extern rlibc=$THESEUS_BASE_DIR/target/x86_64-theseus/release/deps/librlibc-4eb1a1ba9385f780.rmeta \
# 	--extern serial_port=$THESEUS_BASE_DIR/target/x86_64-theseus/release/deps/libserial_port-ce2d7a263b9ad06d.rmeta \
# 	--emit=obj -C debuginfo=2 -C code-model=large -C relocation-model=static -D unused-must-use -Z merge-functions=disabled -Z share-generics=no \
# 	--sysroot /home/kevin/.xargo
