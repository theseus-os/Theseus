sudo ip link add br0 type bridge
sudo ip tuntap add dev tap0 mod tap user $(whoami)
sudo ip link set tap0 master br0
#sudo ip addr add 192.168.69.100/24 dev tap0
sudo ip link set dev br0 up
sudo ip link set dev tap0 up
