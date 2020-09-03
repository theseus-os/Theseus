# Runqueue state spill evaluation

This folder contains necessary components to measure state spill evaluation presented in figure 5.

## Description
This folder contain two pre-built Theseus iso images. 

1. spill_free.iso is compiled using default settings with flags
   `make iso THESEUS_CONFIG="rq_eval"`

2. spillful.iso is compiled with state spill enabled. 

   `make iso THESEUS_CONFIG="rq_eval runqueue_spillful"`

Both iso images are modified to run [rq_eval](../../applications/rq_eval) application upon booting. Each image would measure the average time to add and remove 100000 tasks to a runqueue and average time to spawn and cleanup 100 tasks. Each test is run 10 times to generate average values.

[script.sh](./script.sh) contains the top level script to run the process. The script runs the two images on a QEMU emulator with cores varying from 2 to 72. The script would analyze the serial outputs of multiple runs and output the results in two tables.

### Dependencies

This evaluation depends on `python3`, `pip3` and `PTable` python package in addition to QEMU emulator. They can be installed as follows

1. To install `python3`

   `sudo apt-get install python3.6`

2. To install `pip3`

   `sudo apt-get install python3-pip`

3. To install `PTable` packages

   `sudo pip3 install PTable`

## Evaluation Process
Run [script.sh](./script.sh) to regenerate the results using given iso images.

As an alternative, to reproduce the results using the main branch. Compile the two iso images using the flags provided above. Upon booting enter the following commands on Theseus terminal.

1. To measure the time to add and remove 100000 tasks

   `rq_eval -s 100000`

2. To measure the time to spawn and clean up 100 tasks

   `rq_eval -w 100`