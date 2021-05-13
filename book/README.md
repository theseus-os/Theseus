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

## See also: Source-level Documentation
For specific details about the source code, e.g., structs, functions, modules, and more, please check out the source-level documentation generated from the inline source code comments by `rustdoc`.