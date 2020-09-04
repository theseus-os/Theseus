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

#Order the tests are run in the script.
mapping=(none s1 s3 s4 r1 s5 r2 r3 r4 r5 r6 s6 s2 s7 none none none)

for test in {1..13}; do
    if grep -q "TEST RESULT ${test} " ./results/output.txt
    then
        echo "Test ${mapping[test]} passed"
    else
        echo "Test ${mapping[test]} failed"
    fi
done


