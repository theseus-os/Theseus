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
We have even created a Docker image that automates the setup process and thus makes it easy to verify Theseus's functionality.

The source code of Theseus is well-documented and the documentation is hosted online for easy access.
We also provide a "book" that explains higher-level concepts in Theseus, though this is less rigorously vetted than Theseus's source-level docs. 

The subfolders in this directory contain scripts and pre-built images that allow others to verify the experiments described in the paper. 

## Results Reproducible 
As mentioned above, each subfolder in this directory corresponds to an experiment in the paper and includes everything necessary to reproduce that experiment. 
