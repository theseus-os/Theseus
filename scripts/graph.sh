### If you don't have cargo graph installed, run this: cargo +nightly install --git https://github.com/kbknapp/cargo-graph --force
if cargo --list | grep graph; then
	cargo graph > /tmp/dot.dot && dot -Tpdf > /tmp/output.pdf /tmp/dot.dot && xdg-open /tmp/output.pdf
else
	echo -e "\nPlease install cargo-graph with the following command:\n\tcargo +nightly install --git https://github.com/kbknapp/cargo-graph --force"
fi
