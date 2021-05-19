# The Theseus OS Book

## Introduction to Theseus
- [Introduction to Theseus OS](ch00-00-introduction.md)


## Design and Structure of Theseus 
- [Design and Structure of Theseus](design.md)
    - [Source Code Repository Organization](source_code_organization.md)
    - [Boot-up Procedure](booting.md)
    - [Safe-language OS Principles](idea.md)
    - [Intralingual Design]() <!-- TODO: intralingual.md -->


## Application Development
- [Developing a Theseus Application](app.md)


## Building and Configuring Theseus
- [The Theseus Build Process](build_process.md)
    - [Configuring Theseus](configuration.md)
    - [`theseus_cargo`: Building Rust Crates Out-of-Tree](rust_builds_out_of_tree.md)


## Experimental Support for C programs
- [Experimental Support for C programs](c_program.md)
    - [Building a C cross compiler for Theseus](building_c_cross_compiler.md)
    - [`tlibc`: Theseus's libc and how it works](c_compilation_tlibc.md)
    - [Compiling and linking C programs](c_compiler_linker.md)


## Overview of Key Subsystems 
- [Overview of Key Subsystems](subsystems.md)
    - [Memory Management]() <!-- TODO: memory.md -->
    - [Task Management]() <!-- TODO: task.md -->
    - [Display and Window Management](display.md)
        - [The Window Manager](window_manager.md)
        - [Creating and Displaying Windows](window_tutorial.md)


## Running Theseus on Real Hardware
- [Running Theseus on VMs & Real Hardware](real_hardware.md)
    - [Running Theseus in a Virtual Machine](virtual_machines.md)
        - [Using PCI device Passthrough on QEMU](pci_passthrough.md)
    - [Booting via USB drive](booting_usb.md)
    - [Booting over the network (PXE)](pxe.md)


## How to Contribute
- [How to Contribute](ch01.md)
    - [`git` Guidlines](git.md)


-------------------

## Theseus Slide Decks
[Papers and Presentations/Slides](papers_presentations.md)

## Link to Theseus README
[Theseus README + Quick Start ↗️](_root_readme.md)
