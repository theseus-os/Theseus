set -e

### the IFACE argument is required, e.g., "eth0", "enp6s0"
if [ -z $1 ]; then 
	echo "Error: expected one argument, the network interface name, e.g., 'eth0', 'enp6s0'"
	exit 1
fi

### The following two variables can be set on the command line if you want to override the default values:
BRIDGE_NAME=${BRIDGE_NAME:-br0}
TAP_NAME=${TAP_NAME:-tap0}
IFACE=$1


echo "Warning: adding the \"$IFACE\" interface to a new bridge \"$BRIDGE_NAME\" will reset its IP and connection."

### create the bridge
sudo ip link add $BRIDGE_NAME type bridge

### create the tap device for the current user
sudo ip tuntap add dev $TAP_NAME mode tap user $(whoami)
### add the tap device to the bridge
sudo ip link set $TAP_NAME master $BRIDGE_NAME

### bring up all the interfaces
sudo ip link set dev $BRIDGE_NAME up
sudo ip link set dev $TAP_NAME up
sudo ip link set dev $IFACE up   #probably already brought up, but doesn't hurt to try again

### remove the IP address from the working network interface
sudo ip addr flush dev $IFACE
### add the working network interface to the bridge
sudo ip link set $IFACE master $BRIDGE_NAME

### since we removed the IP address above, we need to get a new IP addr for the bridge (using DHCP)
echo "Finished setting up the bridge and its interfaces, now obtaining a new IP address via DHCP..."
sudo dhclient $BRIDGE_NAME
echo "Done!"

