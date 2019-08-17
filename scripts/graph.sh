### If you don't have cargo deps installed, run this: cargo +nightly install --git https://github.com/kbknapp/cargo-graph --force
if cargo --list | grep deps; then
	cargo deps --include-orphans --no-transitive-deps  | dot -Tpdf > /tmp/graph.pdf && xdg-open /tmp/graph.pdf 
else
	echo -e "\nPlease install cargo-deps with the following command:\n\tcargo install cargo-deps --force"
fi
