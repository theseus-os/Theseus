#!/bin/bash

rm -rf results
mkdir results

for i in {0..10..1}
do
	ran_num_1=$((RANDOM & 0x002))
	ran_num_2=$((RANDOM & 0xfff))
	ran_num_3=$((RANDOM & 0xfff))
	ran_num_4=$ran_num_1*0x1000000
	ran_num_5=$ran_num_2*0x1000
	ran_num_6=$ran_num_4+$ran_num_5+$ran_num_3

	cor_val_1=$((RANDOM*131076))
	cor_val_2=$((RANDOM%131076))
	cor_val=$(($cor_val_1+$cor_val_2))
	
	if [[ $(( i % 5 )) == 0 ]]
	then
 		# full_memory word corruption text
		add_val=$(( 0xFFFFFF8000000000 + ($ran_num_6) ))
		add_val=`printf "0x%x\n" $add_val`
		echo $add_val

		echo "set \$address=$add_val" > gdb_commands
		echo "set \$corrupt=$cor_val" >> gdb_commands
		cat gdb_commands_corrupt >> gdb_commands

		xterm -title "App 1" -e "qemu-system-x86_64 -cdrom theseus-x86_64.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp 4 -net none -cpu Broadwell -S >> ./results/output_${add_val}.txt -S" &
	elif [[ $(( i % 5 )) == 1 ]]
	then
	    # full_memory word corruption other
		add_val=$(( 0xFFFFFFE000000000 + ($ran_num_6) ))
		add_val=`printf "0x%x\n" $add_val`
		echo $add_val

		echo "set \$address=$add_val" > gdb_commands
		echo "set \$corrupt=$cor_val" >> gdb_commands
		cat gdb_commands_corrupt >> gdb_commands

		xterm -title "App 1" -e "qemu-system-x86_64 -cdrom theseus-x86_64.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp 4 -net none -cpu Broadwell -S >> ./results/output_${add_val}.txt -S" &
	elif [[ $(( i % 5 )) == 2 ]]
	then
	    # bit corruption text
		add_val=$(( 0xFFFFFF8000000000 + ($ran_num_6) ))
		add_val=`printf "0x%x\n" $add_val`
		echo $add_val

		echo "set \$address=$add_val" > gdb_commands
		echo "set \$corrupt=$cor_val" >> gdb_commands
		cat gdb_commands_bit_flip >> gdb_commands

		xterm -title "App 1" -e "qemu-system-x86_64 -cdrom theseus-x86_64.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp 4 -net none -cpu Broadwell -S >> ./results/output_${add_val}_bit_flip.txt -S" &
	elif [[ $(( i % 5 )) == 3 ]]
	then
	    # bit corruption other
		add_val=$(( 0xFFFFFFE000000000 + ($ran_num_6) ))
		add_val=`printf "0x%x\n" $add_val`
		echo $add_val

		echo "set \$address=$add_val" > gdb_commands
		echo "set \$corrupt=$cor_val" >> gdb_commands
		cat gdb_commands_bit_flip >> gdb_commands

		xterm -title "App 1" -e "qemu-system-x86_64 -cdrom theseus-x86_64.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp 4 -net none -cpu Broadwell -S >> ./results/output_${add_val}_bit_flip.txt -S" &
	else
	    # skip instruction text
		add_val=$(( 0xFFFFFF8000000000 + ($ran_num_6) ))
		add_val=`printf "0x%x\n" $add_val`
		echo $add_val

		echo "set \$address=$add_val" > gdb_commands
		echo "set \$corrupt=$cor_val" >> gdb_commands
		cat gdb_commands_skip >> gdb_commands

		xterm -title "App 1" -e "qemu-system-x86_64 -cdrom theseus-x86_64.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp 4 -net none -cpu Broadwell -S >> ./results/output_${add_val}_skip.txt -S" &
	fi

	sleep 1
	rust-gdb ./nano_core-x86_64.bin -x ./gdb_commands &
	sleep 45
	qemu_id=$(pidof qemu-system-x86_64)
	kill -9 $qemu_id
	pkill xterm
	pkill rust-gdb
	pkill gdb
done


# Evaluvate the results.

rm -rf results_no_faults
mkdir results_no_faults
cd results
grep -rl --null --include '*.txt' "watch_thread : no faults" . | xargs -0r cp -t ../results_no_faults

cd ..
rm -rf results_temp
mkdir results_temp
cd results
grep -rl --null --include '*.txt' "working_thread : no faults" . | xargs -0r cp -t ../results_temp

cd ../results_temp
grep -rL --null --include '*.txt' "- FAULT LOG -" . | xargs -0r cp -t ../results_no_faults
cd ..
rm -rf results_temp

rm -rf results_recovered
mkdir results_recovered
cd results
grep -rl --null --include '*.txt' "recovered from detected faults" . | xargs -0r cp -t ../results_recovered

cd ..
rm -rf results_temp
mkdir results_temp
cd results
grep -rL --null --include '*.txt' "recovered from detected faults" . | xargs -0r cp -t ../results_temp

cd ..
rm -rf results_failed
mkdir results_failed
cd results_temp
grep -rL --null --include '*.txt' "no faults" . | xargs -0r cp -t ../results_failed
cd ..
rm -rf results_temp

echo Recovered
ls ./results_recovered | wc -l

echo Failed to Recover
ls ./results_failed | wc -l
