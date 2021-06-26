# Published Documentation

This directory contains files used to publish Theseus's documentation online, which is realized using GitHub Pages at [https://theseus-os.github.io/Theseus](https://theseus-os.github.io/Theseus).

Any file in this directory is included in the published output (e.g., `index.html`), unless it is explicitly added to the local `.gitignore` file here.

## Automatically Generated Directories

When building Theseus's book and source code documentation (rustdoc) automtically using GitHub Actions, the `book/` and `doc/` directories are  generated here. Those directories are ignored by git in the top-level `.gitignore` file.
