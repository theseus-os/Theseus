# Runqueue state spill evaluation

This folder contain necessary components to measure sate spill evaluation presented in figure 5.

## Description
This folder contain two pre-built Theseus iso images where spill_free.iso is compiled using default settings and spillful.iso is compiled with state spill enabled. Both iso images are modified to run [rq_eval](../../applications/rq_eval) application upon booting. Each image would measure the average time to add and remove 100000 tasks to a runqueue and average time to spawn and cleanup 100 tasks. Each test is run 10 times to generate average values.

[script.sh](./script.sh) contains the top level script to run the process. The script runs the two images on qemu emulator with cores varying from 2 to 72. The script would analyze the serial outputs of multiple runs and output the results in two tables.

## Evaluation Process
Run [script.sh](./script.sh) 
