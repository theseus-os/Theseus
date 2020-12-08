RUSTFLAGS="--emit=obj -C debuginfo=0 -C codegen-units=1" \
    RUST_TARGET_PATH="/home/kevin/Dropbox/Theseus/cfg" \
    xargo build  --release \
    --target x86_64-theseus \
    --verbose 
