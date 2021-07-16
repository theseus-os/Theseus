# The Theseus OS Book

## Introduction to Theseus
[Introduction to Theseus OS](index.md)


## Design and Structure of Theseus 
- [Design and Structure of Theseus](design/design.md)
    - [Source Code Repository Organization](design/source_code_organization.md)
    - [Boot-up Procedure](design/booting.md)
    - [Safe-language OS Principles](design/idea.md)
    - [Intralingual Design]() <!-- TODO: intralingual.md -->


## Application Development
- [Developing a Theseus Application](app/app.md)


## Building and Configuring Theseus
- [The Theseus Build Process](building/building.md)
    - [Configuring Theseus](building/configuration.md)
    - [`theseus_cargo`: Building Rust Crates Out-of-Tree](building/rust_builds_out_of_tree.md)


## Experimental Support for C programs
- [Experimental Support for C programs](c/programs.md)
    - [Building a C cross compiler for Theseus](c/cross_compiler.md)
    - [tlibc: Theseus's libc and how it works](c/tlibc.md)
    - [Compiling and linking C programs](c/compiler_linker.md)


## Overview of Key Subsystems 
- [Overview of Key Subsystems](subsystems/subsystems.md)
    - [Memory Management]() <!-- TODO: memory.md -->
    - [Task Management]() <!-- TODO: task.md -->
    - [Display and Window Management](subsystems/display/display.md)
        - [The Window Manager](subsystems/display/window_manager.md)
        - [Creating and Displaying Windows](subsystems/display/window_tutorial.md)


## Running Theseus on Real Hardware
- [Running Theseus on Virtual Machines & Real Hardware](running/running.md)
    - [Running Theseus in a Virtual Machine](running/virtual_machine/virtual_machine.md)
        - [Using PCI device Passthrough on QEMU](running/virtual_machine/pci_passthrough.md)
    - [Booting via USB drive](running/usb.md)
    - [Booting over the network (PXE)](running/pxe.md)


## How to Contribute
- [How to Contribute](contribute/contribute.md)
    - [Git Guidelines](contribute/git.md)


-------------------

## Theseus Slide Decks
[Papers and Presentations/Slides](misc/papers_presentations.md)

## Link to Theseus README
[Theseus README + Quick Start ↗️](misc/quick_start.md)
