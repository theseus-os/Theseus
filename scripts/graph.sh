### If you don't have cargo graph installed, run this: cargo +nightly install --git https://github.com/kbknapp/cargo-graph --force
cargo graph > /tmp/dot.dot && dot -Tpdf > /tmp/output.pdf /tmp/dot.dot && evince /tmp/output.pdf
