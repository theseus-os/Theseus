#!/bin/bash

# check the dependencies
# No dependencies


# initial setup

rm -rf results
mkdir results


# Run the main script


qemu-system-x86_64 -cdrom theseus-x86_64.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp 4 -net none -cpu Broadwell >> ./results/output.txt &

sleep 1200

qemu_id=$(pidof qemu-system-x86_64)
kill -9 $qemu_id

# Extract results from log

for test in {1..13}; do
    grep "TEST RESULT ${test}"
done

