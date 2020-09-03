#!/bin/bash

# initial setup

rm -rf results
mkdir results


core_count=(2 4 8 16 32 64 72)


# Run the state spill free version for 2, 4, 8, 16, 32, 64, 72 cores

for core in ${core_count[@]}; do
    qemu-system-x86_64 -cdrom spill_free.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp $core -drive format=raw,file=random_data2.img,if=ide -net none -cpu Broadwell >> ./results/spill_free_${core}.txt &

    sleep 300

    qemu_id=$(pidof qemu-system-x86_64)
	kill -9 $qemu_id
done

# Run the spillful version for 2, 4, 8, 16, 32, 64, 72 cores

for core in ${core_count[@]}; do
    qemu-system-x86_64 -cdrom spillful.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp $core -drive format=raw,file=random_data2.img,if=ide -net none -cpu Broadwell >> ./results/spillful_${core}.txt &

    sleep 300

    qemu_id=$(pidof qemu-system-x86_64)
	kill -9 $qemu_id
done

# Extract results from log

for core in ${core_count[@]}; do
    grep "Log Single" ./results/spill_free_${core}.txt >> ./results/spill_free_single_${core}.csv
    grep "Log Whole" ./results/spill_free_${core}.txt >> ./results/spill_free_whole_${core}.csv
    grep "Log Single" ./results/spillful_${core}.txt >> ./results/spillful_single_${core}.csv
    grep "Log Whole" ./results/spillful_${core}.txt >> ./results/spillful_whole_${core}.csv
done

# Calculate median and standard deviation from the log
# please install PTable python package to display output if not available"sudo pip3 install PTable"
python3 calculate.py
