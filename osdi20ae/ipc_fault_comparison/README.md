IPC fault comparison between Theseus and MINIX3

This folder contains necessary components to evaluate fault recovery in IPC channels of Theseus and MINIX3.

## Description
This folder contains a Theseus iso image generated with settings:

   `make iso THESEUS_CONFIG="unwind_exceptions"`

The iso image contains a modified rendezvous channel with the listed faults injected successively.

[script.sh](./script.sh) contains the top-level script to run the process. The script runs the iso image on a QEMU emulator. The script analyzes the serial console/log output of the run.

[modified_files](./modified_files/) contains the modified rendezvous channel and top level application. This is not merged directly to the mainline since the version contains injected faults.  [script_modify.sh](./script_modify.sh) replaces the original files with the fault injected version to rebuild the iso image. 

## Table of Faults 

```markdown
| Fault ID | Fault                                                      | Theseus Response   | Minix 3 Response       |
|----------|------------------------------------------------------------|--------------------|------------------------|
| s1       | Random page fault induced in sender routine                | Recover by restart | kernel panic -> Reboot |
| s2       | message pointer sent to null in sending routine            | Recover by restart | Message lost           |
| s3       | message pointer set to unmapped address in sending routine | Recover by restart | kernel panic -> Reboot |
| s4       | Sender pointer set to unmapped process in send routine     | Recover by restart | kernel panic -> Reboot |
| s5       | Wait queue set to unmapped address in send routine         | Recover by restart | kernel panic -> Reboot |
| s6       | Empty channel not in initial state when send begin         | Hung Task          | kernel panic -> Reboot |
| s7       | State not updated after transmitting message by sender     | Hung Task          | Message lost           |
| r1       | Receive pointer set to unmapped process in receive routine | Recover by restart | kernel panic -> Reboot |
| r2       | Empty channel not in initial state when receive begin      | Recover by restart | kernel panic -> Reboot |
| r3       | Random page fault induced in receiver routine              | Recover by restart | kernel panic -> Reboot |
| r4       | Receive msg buffer set to null                             | Recover by restart | kernel panic -> Reboot |
| r5       | Wait queue set to unmapped address in receiver routine     | Recover by restart | kernel panic -> Reboot |
| r6       | An empty slot marked as occupied                           | Recover by restart | kernel panic -> Reboot |
```



## Evaluation Process on Theseus

Run [script.sh](./script.sh) to generate the results, as described above.

If one wanted to generate the same results from source without prebuilt images (reproduce the results), simply run the `./script_modify` and afterwards build theseus using the flags given above. Upon booting Theseus, run the following commands in Theseus's shell.

   `ipc_faults --<FAULT_ID>`

e.g.,

   `ipc_faults --r3` to generate fault listed as `r3` in the table

## Evaluation Process on MINIX 3

Modified source code for MINIX 3 is available at  [minix_osdi_ae](https://github.com/theseus-os/minix_osdi_ae). The repository contain a separate branch for each fault where each branch has the same name as that fault id ( e.g., `r1`). Use the following steps to reproduce each fault. 

1. Clone the repository

   `git clone git@github.com:theseus-os/minix_osdi_ae.git`

2. Checkout the required branch (e.g., to generate `s1` fault check `s1` branch)

   `git checkout -b s1 origin/s1`

3. Built MINIX 3

   `cd minix_osdi_ae`

   `bash ./releasetools/x86_hdimage.sh` 

4. Run the build MINIX 3 version

   `cd ../obj.i386/destdir.i386/boot/minix/.temp && qemu-system-i386 --enable-kvm -m 256M -kernel kernel -initrd "mod01_ds,mod02_rs,mod03_pm,mod04_sched,mod05_vfs,mod06_memory,mod07_tty,mod08_mib,mod09_vm,mod10_pfs,mod11_mfs,mod12_init" -hda ../../../../../minix_osdi_ae/minix_x86.img -serial file:../../../../../minix_osdi_ae/serial.out -append "rootdevname=c0d0p0 cttyline=0"`

5. Login as root in MINIX 3 shell

6. Press x and enter in MINIX 3 shell. The fault chosen by branch will be injected to the IPC channel between input and tty processes.

7. Output is logged at `serial.out`. For all except `s1` and `s6` the log will show a panic followed by restart.

8. Repeat from step 2 to regenerate other faults.