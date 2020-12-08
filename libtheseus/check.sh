RUST_TARGET_PATH="/home/kevin/Dropbox/Theseus/cfg"  \
	RUSTFLAGS="--emit=obj -C debuginfo=2 -C code-model=large -C relocation-model=static -D unused-must-use -Z merge-functions=disabled -Z share-generics=no" \
	xargo check  --release  \
	--target x86_64-theseus	--verbose 


	## We cannot use "--build-plan", because it's getting removed soon. 
	# --build-plan -Z unstable-options > build_plan_xargo

