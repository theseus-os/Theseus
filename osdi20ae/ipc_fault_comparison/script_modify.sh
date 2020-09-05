#!/bin/bash

# check the dependencies
# No dependencies


# initial copying of files

rm -rf ../../kernel/rendezvous
rm -rf ../../applications/ipc_faults
cp -r modified_files/rendezvous ../../kernel/rendezvous
cp -r modified_files/ipc_faults ../../applications/ipc_faults

