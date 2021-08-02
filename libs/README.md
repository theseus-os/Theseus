# `libs/`: Third-party Libraries

This directory contains libraries that are used in and/or customized by Theseus but are considered "third-party".
The general idea is that these libraries are not Theseus-specific and could be refactored out into their own standalone projects easily.

As such, and most importantly, libraries in `libs/` **must not depend** on any crates in the Theseus kernel (in `kernel/`). 

Some of these folders are git submodules (separate repositories), while others are included directly in the main Theseus repository itself. 

