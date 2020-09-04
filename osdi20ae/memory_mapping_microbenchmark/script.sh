# check the dependencies
if command -v python3 >/dev/null 2>&1 
then 
	echo Python 3 is installed. Test will continue
else
	echo Python 3 not found.
	echo Please install python3 using \"sudo apt-get install python3.6\" 
	exit
fi

if command -v pip3 >/dev/null 2>&1 
then 
	echo pip3 is installed. Test will continue
else
	echo pip3 not found.
	echo Please install pip3 using \"sudo apt-get install python3-pip\" 
	exit
fi

if python3 -c 'import prettytable' 2>/dev/null 
then 
	echo PTable is installed. Test will continue
else
	echo PTable not found.
	echo Please install PTable using \"sudo pip3 install PTable\" 
	exit
fi


for i in $(seq 0 9)
do
	qemu-system-x86_64 -cdrom theseus-x86_64.iso -no-reboot -no-shutdown -s -m 1G -serial stdio -smp 8 -net none -cpu host -accel kvm > ./results/results_${i}.txt &
	qemu_id=$(pidof qemu-system-x86_64)

	while sleep 1m
	do
		if fgrep --quiet "COMPLETED MEM_MAP EVALUATION" "./results/results_${i}.txt"
		then
			kill -9 $qemu_id
			break
		fi
	done
done

python3 parse.py