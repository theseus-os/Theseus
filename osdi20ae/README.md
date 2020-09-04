# OSDI 2020 Artifact Evaluation

Hello, and thank you for taking the time to evaluate our artifacts for our OSDI 2020 submission! We really do appreciate it.  

Note: the real name of our system is **Theseus**, but it was changed during the submission to "Agora" for anonymity.

## Artifacts Available
All artifacts mentioned in the paper are preserved here as a public repository on GitHub.
Theseus is open-source and licensed under the MIT license for public use.
In addition, we ensure that all source code outside of this repository is either publicly available on our [theseus-os](https://github.com/theseus-os) GitHub organization page or is hosted on [crates.io](https://crates.io/).

## Artifacts Functional
Theseus OS is a functional system that can be built by anyone, and everything mentioned in the paper is contained within this repository. 
The [top-level readme](../README.md) provides detailed instructions on setting up a build environment, building, running, and debugging Theseus.
We have even created a Docker image that automates the setup process and thus makes it easy to build, run, and test Theseus's functionality.

Theseus as described in our OSDI submission is built using a configuration in which all entities are loaded dynamically at runtime rather than linked together statically at build time.
This configuration can be built and run using the `make loadable` target, or with any other `make` command so long as `THESEUS_CONFIG="loadable"` is provided as well, e.g., `make iso THESEUS_CONFIG="loadable"`. 
When running `make loadable`, one can observe log statements in the serial console (which is the same terminal you ran QEMU in) that describe each crate being loaded and linked into the running system dynamically.
Note that entities are termed *cellules* in the paper submission to distinguish the runtime form from the build-time form, but are simply referred to as crates throughout the source code. 


The source code of Theseus is well-documented and the documentation is hosted online for easy access.
We also provide a "book" that explains higher-level concepts in Theseus, though this is a continuous work-in-progress that is less rigorously vetted than Theseus's source-level docs. 

The subfolders in the [osdi20ae](../osdi20ae) directory contain scripts and pre-built images that allow others to verify the experiments described in the paper. 

## Results Reproducible 
As mentioned above, each subfolder in [osdi20ae](../osdi20ae) corresponds to an experiment in the paper and includes everything necessary to reproduce that experiment. 
