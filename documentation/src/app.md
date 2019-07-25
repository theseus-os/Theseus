# Kernel app: support and development

One of the unusual features of Theseus, compared to main-stream operating systems like Linux, is that safe applications can be loaded into the same address space of the kernel and run at the kernel privilege. Below we provide information about how such kernel apps are supported by Theseus and are developed.


## Dynamic Linking and Loading of app crates
Kernal apps are binaries that are loaded into the kernl address space and run at the kernel privilege. In Theseus,
a kernel app is just a collection of crates with a single entry point defined as `fn main()`.

##

