set -e

### The following three variables can be set on the command line if you want to override the default values:
BRIDGE_NAME=${BRIDGE_NAME:-br0}
TAP_NAME=${TAP_NAME:-tap0}
NW_IFACE=${NW_IFACE:-enp6s0}


echo "Warning: adding the \"$NW_IFACE\" interface to a new bridge \"$BRIDGE_NAME\" will reset its IP and connection."

### create the bridge
sudo ip link add $BRIDGE_NAME type bridge

### create the tap device for the current user
sudo ip tuntap add dev $TAP_NAME mode tap user $(whoami)
### add the tap device to the bridge
sudo ip link set $TAP_NAME master $BRIDGE_NAME

### bring up all the interfaces
sudo ip link set dev $BRIDGE_NAME up
sudo ip link set dev $TAP_NAME up
sudo ip link set dev $NW_IFACE up   #probably already brought up, but doesn't hurt to try again

### remove the IP address from the working network interface
sudo ip addr flush dev $NW_IFACE
### add the working network interface to the bridge
sudo ip link set $NW_IFACE master $BRIDGE_NAME

### since we removed the IP address above, we need to get a new IP addr for the bridge (using DHCP)
echo "Finished setting up the bridge and its interfaces, now obtaining a new IP address via DHCP..."
sudo dhclient $BRIDGE_NAME
echo "Done!"

