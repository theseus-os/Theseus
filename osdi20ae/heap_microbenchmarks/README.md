# Heap Microbenchmarks
This folder includes all necessary materials to run the heap microbenchmarks on Theseus.
## Description
The results comparing three different versions of the heap are given in Table 2 of the paper. The microbenchmarks we run are:
- **threadtest:**
- **shbench:** 

In each of these benchmarks, the OS function being measured is run for at least a thousand iterations, after which the mean and standard deviation are calulated.  
In the subfolders **/loadable** and **/static** we have provided pre-built images of 2 configurations of Theseus on which we ran these benchmarks. More information is given below of how to compile and run the two versions.

## Running the Benchmarks
To run all three versions of Theseus and obtain the results for all benchmarks, run **script.sh**.  
`./script.sh`  
A table with the results for each benchmark will be printed out at the end.

Another way is to build the images as given below, launch Theseus, and run the following commands in the Theseus terminal:  
`heap_eval --threadtest`    
`heap_eval --shbench`    

## Versions
### Unsafe Heap
The version of Theseus using the unsafe heap can be built for these benchmarks using the command:

`make iso THESEUS_CONFIG+=unsafe_heap`

### Partially Safe Heap
The version of Theseus using the partially safe heap can be built for these benchmarks using the command:

`make iso`

### Safe Heap
The version of Theseus using the safe heap can be built for these benchmarks using the command:
`make iso THESEUS_CONFIG+=safe_heap`
