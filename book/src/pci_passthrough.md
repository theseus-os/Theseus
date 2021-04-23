# PCI passthrough of devices using QEMU
PCI passthrough can be used to give direct access of a physical PCI device to a guest OS. 
The following instructions are a combination of [this](https://www.ibm.com/docs/en/linux-on-systems?topic=vfio-host-setup) guide on host setup for VFIO passthrough devices and [this](https://www.kernel.org/doc/Documentation/vfio.txt) kernel documentation on VFIO.

There are three main steps to prepare a device for PCI passthrough:
1. Find device information
2. Detach device from current driver
3. Attach device to VFIO driver

Once these steps are completed, QEMU can be started up using the command:  
`make run host=yes pci_dev=59:00.0`

### Finding device information
First, using `lspci -vnn`, find the slot information, kernel driver in use for the device, vendor ID and device code for the device you want to use.
This is our output for a Mellanox ethernet card we want to access.
```
59:00.0 Ethernet controller [0200]: Mellanox Technologies MT28800 Family [ConnectX-5 Ex] [15b3:1019]
	Subsystem: Mellanox Technologies MT28800 Family [ConnectX-5 Ex] [15b3:0008]
	Flags: bus master, fast devsel, latency 0, IRQ 719, NUMA node 1
	Memory at 39bffe000000 (64-bit, prefetchable) [size=32M]
	Expansion ROM at bf200000 [disabled] [size=1M]
	Capabilities: <access denied>
	Kernel driver in use: mlx5_core
	Kernel modules: mlx5_core
```

### Detach device from current driver
To detach the device from the kernel driver, run the following command, filling in the slot_info and driver_name from information you retrieved in the previous step.
``` 
`echo $(slot_info) > /sys/bus/pci/drivers/$(driver_name)/unbind`
```
e.g. we ran:
`echo 0000:59:00.0 > /sys/bus/pci/drivers/mlx5_core/unbind`

If you run `lspci -v` now, you'll see that a kernel driver is longer mentioned for this device

### Attach device to VFIO driver
First, load the VFIO driver:
`modprobe vfio-pci`

To attach the new driver run the following command, filling in the vendor_id and device_code from information you retrieved in the first step.
```
`echo $(vendor_id) $(device_code) > /sys/bus/pci/drivers/vfio-pci/new_id`
```
e.g.
`echo 15b3 1019 > /sys/bus/pci/drivers/vfio-pci/new_id`

### Note: access for unprivileged users
To give access to a unprivileged user to this VFIO device, find the IOMMU group the device belongs to:
```
`readlink /sys/bus/pci/devices/$(slot_info)/iommu_group`
```
e.g. 
`readlink /sys/bus/pci/devices/0000:59:00.0/iommu_group`

for which we got the output:
```
../../../../kernel/iommu_groups/74
```
where 74 is the group number.

Then give access to the user: 
`chown $(user) /dev/vfio/$(group_no)`
