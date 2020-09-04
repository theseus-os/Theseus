# Runqueue state spill evaluation

This folder contains necessary components to measure the runqueue/task management-related state spill evaluation presented in figure 5.

## Description
This folder contain two pre-built Theseus iso images. 

1. `spill_free.iso` is the standard Theseus configuration, compiled with default settings:

   `make iso THESEUS_CONFIG="rq_eval"`

2. `spillful.iso` is a configuration of Theseus in which runqueue/scheduling state is spilled into the task struct, compiled with the following options:

   `make iso THESEUS_CONFIG="rq_eval runqueue_spillful"`

Theseus in both images has been modified to automatically run the [rq_eval](../../applications/rq_eval) application upon booting, for the sake of ease and convenience. Each invocation of `rq_eval` measures the average time to add and remove 100000 tasks to/from a runqueue, and the average time to spawn and cleanup tasks. Each test is run 10 times to generate average values.

[script.sh](./script.sh) contains the top-level script to run the process. The script runs the two images on a QEMU emulator with a differing number of CPU cores, varying from 2 to 72. The script analyzes the serial console/log outputs of these multiple runs and then outputs the results in two tables, again for convenience.

### Dependencies

This evaluation depends on `python3`, `pip3` and `PTable` python package in addition to QEMU emulator. They can be installed as follows:

1. To install `python3`

   `sudo apt-get install python3.6`

2. To install `pip3`

   `sudo apt-get install python3-pip`

3. To install `PTable` packages

   `sudo pip3 install PTable`

## Evaluation Process
Run [script.sh](./script.sh) to generate the results, as described above.

If one wanted to generate the same results from source without prebuilt images (reproduce the results using the main branch), simply compile two versions of Theseus using the flags described above. Upon booting Theseus, run the following commands in Theseus's shell.

1. To measure the time to add and remove 100000 tasks

   `rq_eval -s 100000`

2. To measure the time to spawn and clean up 1000 tasks

   `rq_eval -w 1000`
