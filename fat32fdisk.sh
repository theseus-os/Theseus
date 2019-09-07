#!/usr/bin/env bash
# Automate creation of a fat32 disk.
#
# The sed script strips off all the comments so that we can 
# document what we're doing in-line with the actual commands
# Note that a blank line (commented as "defualt" will send a empty
# line terminated with a newline to take the fdisk default.
# Commands adapted from here: http://fejlesztek.hu/create-a-fat-file-system-image-on-linux/
# See https://superuser.com/questions/332252/how-to-create-and-format-a-partition-using-a-bash-script/1132834
#  as well (used for the sed trick).
sed -e 's/\s*\([\+0-9a-zA-Z]*\).*/\1/' <<EOF| fdisk ./fat32.img
	o # clear the in memory partition table
	n # new partition
	p # primary partition
	1 # partition number 1
	  # default - start at beginning of disk 
	  # default - continue to end of disk
	t # Change table to 
	c # Code for fat32
	w # sync to disk and end
EOF
