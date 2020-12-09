#!/bin/bash
set -e

# capture all output to a file
# script -e .script_output

THESEUS_XARGO_PATH="../tools/theseus_xargo"

export RUST_BACKTRACE=1

### Note: the "theseus_xargo" tool must be installed locally instead of invoked via `cargo run` 
cargo install --force --path=$THESEUS_XARGO_PATH --root=$THESEUS_XARGO_PATH

RUST_TARGET_PATH="/home/kevin/Dropbox/Theseus/cfg"  \
	RUSTFLAGS="--emit=obj -C debuginfo=2 -C code-model=large -C relocation-model=static -D unused-must-use -Z merge-functions=disabled -Z share-generics=no" \
	$THESEUS_XARGO_PATH/bin/theseus_xargo --input ../target/x86_64-theseus/release/deps \
	build --color=always --release \
	--target x86_64-theseus
	


# RUST_TARGET_PATH="/home/kevin/Dropbox/Theseus/cfg"  \
# 	RUSTFLAGS="--emit=obj -C debuginfo=2 -C code-model=large -C relocation-model=static -D unused-must-use -Z merge-functions=disabled -Z share-generics=no" \
# 	xargo build  --release  --verbose -vv \
# 	--target x86_64-theseus	


	# \
	# | tee .build_output \
	# 2> >(grep -vw 'Running\|^+') \

	## We cannot use "--build-plan", because it's getting removed soon. 
	# --build-plan -Z unstable-options > build_plan_xargo


# RUST_TARGET_PATH="/home/kevin/Dropbox/Theseus/cfg" \
# 	rustc --crate-name libtheseus src/lib.rs  --crate-type lib \
# 	--emit=dep-info,metadata,link \
# 	-C opt-level=3 -C embed-bitcode=no -C codegen-units=1 -C metadata=43462c60d48a531a -C extra-filename=-43462c60d48a531a \
# 	--out-dir /home/kevin/Dropbox/Theseus/libtheseus/target/x86_64-theseus/release/deps \
# 	--target x86_64-theseus \
# 	-L dependency=/home/kevin/Dropbox/Theseus/target/x86_64-theseus/release/deps \
# 	--extern rlibc=/home/kevin/Dropbox/Theseus/target/x86_64-theseus/release/deps/librlibc-4eb1a1ba9385f780.rmeta \
# 	--extern serial_port=/home/kevin/Dropbox/Theseus/target/x86_64-theseus/release/deps/libserial_port-ce2d7a263b9ad06d.rmeta \
# 	--emit=obj -C debuginfo=2 -C code-model=large -C relocation-model=static -D unused-must-use -Z merge-functions=disabled -Z share-generics=no \
# 	--sysroot /home/kevin/.xargo