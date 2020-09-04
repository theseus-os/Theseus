qemu-system-x86_64 -cdrom loadable/theseus-x86_64.iso -no-reboot -no-shutdown -s -m 1G -serial stdio -smp 8 -net none -cpu Broadwell > ./loadable/results.txt
sleep 180
qemu_id=$(pidof qemu-system-x86_64)
    kill -9 $qemu_id

qemu-system-x86_64 -cdrom static/theseus-x86_64.iso -no-reboot -no-shutdown -s -m 1G -serial stdio -smp 8 -net none -cpu Broadwell > ./static/results.txt
sleep 180
qemu_id=$(pidof qemu-system-x86_64)
    kill -9 $qemu_id

