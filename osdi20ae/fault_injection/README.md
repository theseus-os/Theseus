# Fault Injection artifacts

This folder contains necessary components to run fault injection experiments on Theseus.

## Description
This folder contain two sub folders, which are two workloads we used during theseus fault injection experiments.

1. `fs` folder contains a pre-built Theseus iso image which runs a file system workload (file open, write and read upon) booting. The image is compiled using 

   `make iso THESEUS_CONFIG=" loadable unwind_exceptions use_crate_replacement use_iterative_replacement loscd_eval"`

2. `itc` folder contains a pre-built Theseus iso image which runs a two tasks communicating with each other using ITC channel upon booting. The image is compiled using 

   `make iso THESEUS_CONFIG=" loadable unwind_exceptions use_crate_replacement use_iterative_replacement loscd_eval use_async_itc"`

Each folder contains a [script.sh](./script.sh) file which is the top-level script to injectfaults into the workload. The scripts inject 3 types of faults (memory word corruption, memory bit flip and instruction skip) while running the workload on QEMU.

In addition to the above two workloads, three more workloads were used during evaluation. They are as follows. 

3. ITC rendezvous channel

4. task spawn

5. graphics rendering

   All three of the above are compiled using the same parameters as `fs` workload and are omitted from our artifacts for simplicity.

### Dependencies

This evaluation depends on `xterm` and `rust-os-gdb`.

1. To install `xterm`

   `sudo apt-get install -y xterm`

2. To install `rust-os-gdb`

   1. First install following packages

      `sudo apt-get install texinfo flex bison python-dev ncurses-dev curl`

   2. Install `rust-os-gdb` to the base Theseus directory

      `cd ../../`

      `curl -sf https://raw.githubusercontent.com/phil-opp/binutils-gdb/rust-os/build-rust-os-gdb.sh | sh`

   3. copy the `rust-os-gdb` to each workload

      `cp rust-os-gdb/bin/rust-gdb osdi20ae/fault_injection/fs/`

      `cp rust-os-gdb/bin/rust-gdb osdi20ae/fault_injection/ipc/`

## Evaluation Process
Run [script.sh](./script.sh) in `fs` and `itc` folders to run fault injection on each workload.

The original set of faults observed during fault_injection phase of our work is listed in [list_of_faults.csv](./list_of_faults.csv).