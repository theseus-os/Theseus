# Heap Microbenchmarks
This folder includes all necessary materials to run the heap microbenchmarks on Theseus.
## Description
In this evaluation we run 2 different microbenchmarks to measure the performance of three different versions of the heap, as given in Table 2 of the paper. The microbenchmarks we run are:
- **threadtest:** allocates and deallocates 100 million 8-byte objects
- **shbench:** allocates and deallocates 19 million randomly sized objects between 1 and 1000 bytes

In the subfolders **/unsafe**, **/partially_safe** and **/safe** we have provided pre-built images of 3 configurations of Theseus on which we ran these benchmarks. More information is given below of how to compile and run the different versions.

## Running the Benchmarks
To run all three versions of Theseus and obtain results for both benchmarks, run **script.sh**.  
`./script.sh`  
A table with the mean and standard deviation for each benchmark will be printed out at the end.

Another way is to build the images as given below, launch Theseus, and run the following commands in the Theseus terminal:  
`heap_eval --threadtest`    
`heap_eval --shbench`  

### Note: QEMU running time
It is recommended to run the benchmark with kvm enabled (as the script currently does). Without kvm it will take a few hours to run the benchmarks on all 3 versions of Theseus on QEMU.

## Versions
### Unsafe Heap
The version of Theseus using the unsafe heap can be built using the command:

`make run THESEUS_CONFIG+=unsafe_heap`

### Partially Safe Heap
The version of Theseus using the partially safe heap can be built using the command:

`make run`

### Safe Heap
The version of Theseus using the safe heap can be built using the command:  
`make run THESEUS_CONFIG+=safe_heap`
