# LMBench Microbenchmarks
This folder includes all necessary materials to run the LMBench benchmarks on Theseus.
## Description
The results for a subset of LMBench microbenchmarks are given in Table 3 of the paper and include:
- null syscall
- context switch
- create process
- memory map
- IPC

In each of these benchmarks, the OS function being measured is run for at least a thousand iterations, after which the mean and standard deviation are calulated.  
In the subfolders **/loadable** and **/static** we have provided pre-built images of 2 configurations of Theseus on which we ran these benchmarks. More information is given below of how to compile and run the two versions.

## Running the Benchmarks
To run both versions of Theseus and obtain the results for all benchmarks, run **script.sh**.  
`./script.sh`  
A table with the results for each benchmark will be printed out at the end.

Another way is to build the images as given below, launch Theseus, and run the following commands in the Theseus terminal:  
`bm --null`  
`bm --ctx`  
`bm --spawn`  
`bm --memory_map`  
`bm --ipc -a -p -b`  

## Versions
### Theseus (Loadable)
The loadable version of Theseus can be built for these benchmarks using the command:

`make iso THESEUS_CONFIG+=bm_map THESEUS_CONFIG+=bm_ipc THESEUS_CONFIG+=loadable host=yes`

### Theseus (Static)
The statically-linked version of Theseus can be built for these benchmarks using the command:

`make iso THESEUS_CONFIG+=bm_map THESEUS_CONFIG+=bm_ipc host=yes`

### Linux (Rust)
The Linux versions of the benchmarks and instructions to run them can be found at:

https://github.com/theseus-os/bm_linux/tree/osdi2020ae