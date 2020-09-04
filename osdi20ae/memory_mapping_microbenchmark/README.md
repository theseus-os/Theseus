# Memory Mapping Microbenchmarks
This folder includes all necessary materials to run the memory mapping benchmark on Theseus.
## Description
The results for the memory mapping microbenchmark are given in Figure 3 of the paper for two mapping mechanisms:
- **MappedPages:** This is the spill-free approach which is Theseus's default way of creating mappings
- **VMAs:** This is the spillful approach where we store mappings in a red-black tree, similar to Linux.

The benchmark maps, remaps and then unmaps a given number of 4KiB mappings using either MappedPages or VMAs. We run each test case multiple times, after which the mean and standard deviation are calulated.  
We have provided a pre-built image of Theseus on which we ran the benchmark. More information is given below of how to compile and run the benchmark.

## Running the Benchmarks
To run Theseus and obtain the results for all test cases, run **script.sh**.  
`sudo ./script.sh`  
A table with the results for each configuration of the benchmark will be printed out at the end.

Another way is to build the image as given below, launch Theseus, and run the following commands in the Theseus terminal:   

`mm_eval -n 100 -s 1` (run the mm_eval benchmark with 100 mappings, each of 1 4-KiB page using the MappedPages approach)  
`mm_eval -n 1000 -s 1`  
`mm_eval -n 10000 -s 1`  
`mm_eval -n 100000 -s 1`  

`mm_eval -n 100 -s 1 -p` (run the mm_eval benchmark with 100 mappings, each of 1 4-KiB page using the VMAs approach)  
`mm_eval -n 1000 -s 1 -p`  
`mm_eval -n 10000 -s 1 -p`  
`mm_eval -n 100000 -s 1 -p`  


## Building Theseus
Theseus can be built for this benchmark using the command:

`make iso THESEUS_CONFIG+=mapper_spillful`
