# Booting Theseus on Real Hardware via PXE
The following instructions are a combination of [this guide on OSTechNix](https://www.ostechnix.com/how-to-install-pxe-server-on-ubuntu-16-04/) to set up PXE for Ubuntu and [this guide by Andrew Wells](https://wellsie.net/p/286/) on how to use any ISO with PXE.

PXE can be used to load Rust onto a target computer that is connected by LAN to the host machine used for development. To set up the host machine for PXE, first make the Theseus ISO by navigating to the directory Theseus is in and running:
`make iso`

Then, you will need to set up a TFTP and DHCP server which the test machine will access.

## Setting up the TFTP Server
First, install all necessary packages and dependencies for TFTP:
`sudo apt-get install apache2 tftpd-hpa inetutils-inetd nasm`
Edit the tftp-hpa configuration file:
`sudo nano /etc/default/tftpd-hpa`
Add the following lines:
```
RUN_DAEMON="yes"
OPTIONS="-l -s /var/lib/tftpboot"
```
Then, edit the `inetd` configuration file by opening the editor:
`sudo nano /etc/inetd.conf`
And adding:
`tftp    dgram   udp    wait    root    /usr/sbin/in.tftpd /usr/sbin/in.tftpd -s /var/lib/tftpboot`

Restart the TFTP server and check to see if it's running:
```sh
sudo systemctl restart tftpd-hpa
sudo systemctl status tftpd-hpa
```

If the TFTP server is unable to start and mentions an in-use socket, reopen the tftp-hpa configuration file,set the line that has `TFTP_ADDRESS=":69"` to be equal to `6969` instead and restart the TFTP server.

## Setting up the DHCP Server
First, install package for DHCP server:
`sudo apt-get install isc-dhcp-server`

Then run `ifconfig` to view available networking devices and find the network device name, e.g., `eth0`.

Edit the `/etc/default/isc-dhcp-server` configuration file and add the network device name from the step above to "INTERFACES". For me, this looks like `INTERFACES="eth0"`.

Configure an arbitrary IP address that will be used in the next step:
`sudo ifconfig <network-device-name> 192.168.1.105`
This command might have to be done each time the computer being used as a server is restarted.

Edit the `/etc/dhcp/dhcpd.conf` file by uncommenting the line `authoritative;` and adding a subnet configuration such as the one below:
```
subnet 192.168.1.0 netmask 255.255.255.0 {
  range 192.168.1.20 192.168.1.30;
  option routers 192.168.1.1;
  option broadcast-address 192.168.1.255;
  default-lease-time 600;
  max-lease-time 7200;
}

allow booting;
allow bootp;
option option-128 code 128 = string;
option option-129 code 129 = text;
next-server 192.168.1.105;
filename "pxelinux.0";
```

Restart the DHCP server and check to see if it's running:
```sh
sudo systemctl restart isc-dhcp-server
sudo systemctl status isc-dhcp-server
```

## Loading the Theseus ISO Into the TFTP Server
In order for the TFTP server to load Theseus, we need the Theseus ISO and a memdisk file in the boot folder. To get the memdisk file first download syslinux which contains it.
```sh
wget https://www.kernel.org/pub/linux/utils/boot/syslinux/syslinux-5.10.tar.gz
tar -xzvf syslinux-*.tar.gz
```

Then navigate to the memdisk folder and compile.
```sh
cd syslinux-*/memdisk
make memdisk
```

Next, make a TFTP boot folder for Theseus and copy the memdisk binary into it along with the Theseus ISO:
```sh
sudo mkdir /var/lib/tftpboot/theseus
sudo cp /root/syslinux-*/memdisk/memdisk /var/lib/tftpboot/theseus/
sudo cp /Theseus/build/theseus-x86_64.iso /var/lib/tftpboot/theseus/
```

Navigate to the PXE configuration file:
`sudo nano /var/lib/tftpboot/pxelinux.cfg/default`
And add Theseus as a menu option by adding the following:
```
label theseus
    menu label Theseus
    root (hd0,0)
    kernel theseus/memdisk
    append iso initrd=theseus/theseus-x86_64.iso raw
```
Finally, restart the DHCP server one more time and make sure it's running:
```sh
sudo systemctl restart isc-dhcp-server
sudo systemctl status isc-dhcp-server
```

On the target computer, boot into the BIOS, turn on Legacy boot mode, and select network booting as the top boot option. Once the target computer is restarted, it should boot into a menu which displays booting into Theseus as an option.

## Subsequent PXE Uses
After setting up PXE the first time, you can run `make pxe` to make an updated ISO, remove the old one, and copy the new one over into the TFTP boot folder. At that point, you should be able to boot that new version of Theseus by restarting the target computer. If there are issues restarting the DHCP server after it worked the first time, one possible solution may be to confirm that the IP address is the one you intended it to be with the command from earlier:
`sudo ifconfig <network-device-name> 192.168.1.105`

