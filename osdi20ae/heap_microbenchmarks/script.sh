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


qemu-system-x86_64 -cdrom partially_safe/theseus-x86_64.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp 4 -net none -cpu Broadwell > ./partially_safe/results.txt &
qemu_id=$(pidof qemu-system-x86_64)

while sleep 10m
do
    if fgrep --quiet "COMPLETED HEAP EVALUATION" "./partially_safe/results.txt"
    then
        kill -9 $qemu_id
        break
    fi
done

qemu-system-x86_64 -cdrom partially_safe/theseus-x86_64.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp 4 -net none -cpu Broadwell  > ./unsafe/results.txt &
qemu_id=$(pidof qemu-system-x86_64)

while sleep 10m
do
    if fgrep --quiet "COMPLETED HEAP EVALUATION" "./unsafe/results.txt"
    then
        kill -9 $qemu_id
        break
    fi
done

qemu-system-x86_64 -cdrom safe/theseus-x86_64.iso -no-reboot -no-shutdown -s -m 512M -serial stdio -smp 4 -net none -cpu Broadwell  > ./safe/results.txt &
qemu_id=$(pidof qemu-system-x86_64)

while sleep 10m
do
    if fgrep --quiet "COMPLETED HEAP EVALUATION" "./safe/results.txt"
    then
        kill -9 $qemu_id
        break
    fi
done

python3 parse.py