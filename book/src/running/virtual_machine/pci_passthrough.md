# PCI passthrough of devices with QEMU
PCI passthrough can be used to allow a guest OS to directly access a physical device.
The following instructions are a combination of [this](https://www.ibm.com/docs/en/linux-on-systems?topic=vfio-host-setup) guide on host setup for VFIO passthrough devices and [this](https://www.kernel.org/doc/Documentation/vfio.txt) kernel documentation on VFIO.

There are three main steps to prepare a device for PCI passthrough:
1. Find device information
2. Detach device from current driver
3. Attach device to VFIO driver

Once these steps are completed, the device slot information can be passed to QEMU using the **vfio** flag. For example, for device 59:00.0, we run:    
```sh
make run vfio=59:00.0
```

### Finding device information
First, run `lspci -vnn` to find the slot information, the kernel driver in use for the device, and the vendor ID and device code for the device you want to use.
Below is sample output for a Mellanox ethernet card we'd like to access using PCI passthrough:
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
To detach the device from the kernel driver, run the following command, filling in the `slot_info` and `driver_name` with values you retrieved in the previous step.
```sh
echo $slot_info > /sys/bus/pci/drivers/$driver_name/unbind
```
In the above example, this would look like:
```sh
echo 0000:59:00.0 > /sys/bus/pci/drivers/mlx5_core/unbind
```

If you run `lspci -v` now, you'll see that a kernel driver is no longer attached to this device.

### Attach device to VFIO driver
First, load the VFIO driver by doing:
```sh
modprobe vfio-pci
```

To attach the new driver, run the following command, filling in the `vendor_id` and `device_code` with values you retrieved in the first step.
```sh
echo $vendor_id $device_code > /sys/bus/pci/drivers/vfio-pci/new_id
```
e.g. `echo 15b3 1019 > /sys/bus/pci/drivers/vfio-pci/new_id`

Now, QEMU can be launched with direct access to the device.

### Return device to the Host OS
To reset the device, you can either reboot the system or return the device to the host OS using the following commands (replacing `$slot_info` with the value previously retrieved):  
```sh
echo 1 > /sys/bus/pci/devices/$slot_info/remove    
echo 1 > /sys/bus/pci/rescan  
```

### Note: access for unprivileged users
To give access to an unprivileged user to this VFIO device, find the IOMMU group the device belongs to:
```sh
readlink /sys/bus/pci/devices/$slot_info/iommu_group
```
for example:
```sh
readlink /sys/bus/pci/devices/0000:59:00.0/iommu_group
```

for which we obtain the output below, in which `74` is the group number:
> ```
> ../../../../kernel/iommu_groups/74
>```

Finally, give access to the current user via this command:
```sh
chown $USER /dev/vfio/$group_number
```

<!-- cspell:ignore iommu, Mellanox, VFIO -->
