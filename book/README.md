# The Theseus OS Book

This directory contains Theseus's book-style documentation, which provides an overview of Theseus design principles, implementation choices, and high-level details about its key components.

You can browse the book directly starting at [SUMMARY.md](src/SUMMARY.md), the table of contents and first chapter.

The book is written in Markdown and uses [mdBook](https://rust-lang-nursery.github.io/mdBook/) to build a nicely-formatted HTML version of the book. 
You can use the top-level [Makefile](../Makefile) to build and view the book by running:
```
make view-book
```

## See also: Source-level Documentation
For specific details about the source code, e.g., structs, functions, modules, and more, please check out the source-level documentation generated from the inline source code comments by `rustdoc`.