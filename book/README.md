# The Theseus OS Book

This directory contains Theseus's book-style documentation, which provides an overview of Theseus design principles, implementation choices, and high-level details about its key components.

You can browse the book directly starting at [SUMMARY.md](src/SUMMARY.md), the table of contents and first chapter.

The book is written in Markdown and uses [mdBook](https://rust-lang-nursery.github.io/mdBook/) to build a nicely-formatted HTML version of the book.

## Building the book

First, install `mdbook`:
```sh
cargo +stable install mdbook
```

You can optionally install a plugin that checks links when building the book:
```sh
cargo +stable install mdbook-linkcheck
```

From the top-level directory, you can use `make` to build and view the book by running:
```sh
make view-book
```

If you have problems installing or using `mdbook`, try to uninstall it, update Rust, and then reinstall it:
```sh
cargo uninstall mdbook
rustup toolchain update stable
cargo +stable install mdbook
```

## Spellcheck

The book source files use [cspell](https://github.com/streetsidesoftware/cspell) for spell checking.
`cspell` can be run through `node` or as a Visual Studio Code [extension](https://github.com/streetsidesoftware/vscode-spell-checker).
Valid words can be added to the `cspell` dictionary by adding the word to the `words` array in the `book\cspell.json` configuration file.
Words to be ignored can be added to the `ignoreWords` array.
Settings that are only applicable to a specific file can be added as inline comments.

## See also: Source-level Documentation
For specific details about the source code, e.g., structs, functions, modules, and more, please check out the source-level documentation generated from the inline source code comments by `rustdoc`.
