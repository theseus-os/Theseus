# Fault Injection artifacts

This folder contains necessary components to run fault injection experiments on Theseus.

## Description
This folder contain two sub folders, which are two workloads we used during theseus fault injection experiments.

1. `fs` folder contains a pre-built Theseus iso image which runs a file system workload (file open, write and read upon) booting. The image is compiled using 

   `make iso THESEUS_CONFIG=" loadable unwind_exceptions use_crate_replacement use_iterative_replacement loscd_eval"`

2. `ipc` folder contains a pre-built Theseus iso image which runs a two tasks communicating with each other using ipc channel (file open, write and read upon) booting. The image is compiled using 

   `make iso THESEUS_CONFIG=" loadable unwind_exceptions use_crate_replacement use_iterative_replacement loscd_eval use_async_ipc"`

Each folder contains a [script.sh](./script.sh) file which is the top-level script to inject faults to that worklaod. The scripts add 3 types of faults (memory word corruption, memory bit flip and instruction skip) while running the workload on QEMU.

In addition to the above 2 workloads 3 more workloads were used during evaluation. They are as follows. 

3. ipc rendezvous channel

4. task spawn

5. graphics rendering

   All 3 of the above are compiled using the same parameters as `fs` workload and are omitted for from artifacts for simplicity.

### Dependencies

This evaluation depends on `xterm`.

1. To install `xterm`

   `sudo apt-get install -y xterm`

## Evaluation Process
Run [script.sh](./script.sh) in `fs` and `ipc` folders to run fault injection on each workload.

The original set of faults observed during fault_injection phase of our work is listed [here](https://docs.google.com/document/d/1k9BeG21ZAiWrMN3-TitT3987zsBd_p0gDCyNzYnWPl4/edit?usp=sharing).