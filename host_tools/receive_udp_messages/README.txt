
 #Setting up host machine if QEMU is used
	sudo ip tuntap add name tap0 mode tap user $USER
	sudo ip link set tap0 up
	sudo ip addr add 192.168.69.100/24 dev tap0

 #Use the setup.sh script with sudo command to setup the host machine


 #Sending UDP packet using socat
 	socat stdio udp4-connect:192.168.66969 <<<"abcdefg"

 #Sample test program that can be used from the connected machine to receive the udp packets
