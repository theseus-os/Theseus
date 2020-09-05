# Fault Injection artifacts

This folder contains the necessary components to run fault injection experiments on Theseus, corresponding to Table 1 in the submission.

## Description
This folder contain two sub folders, one for each of the two workloads we used during Theseus fault injection experiments.

1. `fs` folder contains a pre-built Theseus iso image which runs a file system workload (file open, read, and write) upon booting. The image is compiled via:

   `make iso THESEUS_CONFIG=" loadable unwind_exceptions use_crate_replacement use_iterative_replacement loscd_eval"`

2. `itc` folder contains a pre-built Theseus iso image which, upon booting, runs two tasks that communicate with each other via Theseus's asynchronous ITC channel. The image is compiled via:

   `make iso THESEUS_CONFIG=" loadable unwind_exceptions use_crate_replacement use_iterative_replacement loscd_eval use_async_itc"`

Each folder contains a [script.sh](./script.sh) file which is the top-level script to inject faults into the workload. These scripts each inject 3 types of faults (memory word corruption, memory bit flip, and instruction pointer skip) while running their workload on QEMU.

In addition to the above two workloads, three more workloads were used during evaluation. They are as follows. 

3. Synchronous rendezvous ITC channel (similar to number 2)

4. Task spawning

5. Graphics rendering

   All three of the above are compiled using the same parameters as the first `fs` workload, and are thus omitted from our artifacts for simplicity.

### Dependencies

This evaluation depends on `xterm` and `rust-os-gdb`, a patched version of GDB that works better with Rust code on QEMU.

1. To install `xterm`:

   `sudo apt-get install -y xterm`

2. To install `rust-os-gdb`:

   1. First install the following packages:

      `sudo apt-get install -y texinfo flex bison python-dev ncurses-dev curl`

   2. Download the `rust-os-gdb` files to the base Theseus directory:

      `cd ../../   #  Go to the Theseus base directory`    

      `curl -sf https://raw.githubusercontent.com/phil-opp/binutils-gdb/rust-os/build-rust-os-gdb.sh | sh`

   3. Copy `rust-os-gdb` to each workload

      `cp rust-os-gdb/bin/rust-gdb osdi20ae/fault_injection/fs/`

      `cp rust-os-gdb/bin/rust-gdb osdi20ae/fault_injection/itc/`

## Evaluation Process
After the above dependency setup is complete, run [script.sh](./script.sh) in both the `fs` and `itc` folders to run the fault injection trials on each workload.

The original set of faults observed during the fault injection experiments that appear in Table 1 of our submission are listed in [list_of_faults.csv](./list_of_faults.csv).
