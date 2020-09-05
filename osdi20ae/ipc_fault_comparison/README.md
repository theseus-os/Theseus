# IPC fault comparison between Theseus and MINIX3

This folder contains necessary components to evaluate fault recovery in IPC channels of Theseus and MINIX3.

## Description
This folder contains a prebuilt Theseus iso image that has a version of Theseus's rendezvous ITC channel with faults (listed below) injected successively.

To quickly run the experiment, see the [Evaluation section below](#Evaluation-Process-on-Theseus).

Other contents include:
* [modified_files](./modified_files/): the modified rendezvous channel and top-level application.     
  This is not present in the mainline Theseus branch since it contains injected faults.
* [script_modify.sh](./script_modify.sh): the script to patch/replace the original files with the fault-injected versions such that the prebuilt image can be built.

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

Run [script.sh](./script.sh) to generate the results; this takes roughly 15-20 minutes. It runs the Theseus iso image on a QEMU emulator and then analyzes the serial console/log output.

### Manual build and run instructions
If one wanted to manually generate the same results from source without prebuilt images, simply patch, build, and run Theseus using the following commands: 
```
./script_modify
cd ../../  # navigate back to Theseus base directory
make run THESEUS_CONFIG="unwind_exceptions"
```

Then, in the Theseus shell, run the following command to generate a fault from the above table: 
```
   ipc_faults --<FAULT_ID>
```
where `<FAULT_ID>` is an entry from the left-most column of the above table, e.g.:
```
   ipc_faults --r3
```

## Evaluation Process on MINIX 3

Modified source code for MINIX 3 is available at  [minix_osdi_ae](https://github.com/theseus-os/minix_osdi_ae). The repository contain a separate branch for each fault where each branch has the same name as that fault id ( e.g., `r1`). Use the following steps to reproduce each fault. 

1. Clone the repository:

   `git clone git@github.com:theseus-os/minix_osdi_ae.git`

2. Checkout the required branch (e.g., to generate `s1` fault, check out the `s1` branch):

   `git checkout -b s1 origin/s1`

3. Build MINIX 3:

   `cd minix_osdi_ae`

   `bash ./releasetools/x86_hdimage.sh` 

4. Run the build MINIX 3 version (remove `--enable-kvm` if your system doesn't support it):

   `cd ../obj.i386/destdir.i386/boot/minix/.temp && qemu-system-i386 --enable-kvm -m 256M -kernel kernel -initrd "mod01_ds,mod02_rs,mod03_pm,mod04_sched,mod05_vfs,mod06_memory,mod07_tty,mod08_mib,mod09_vm,mod10_pfs,mod11_mfs,mod12_init" -hda ../../../../../minix_osdi_ae/minix_x86.img -serial file:../../../../../minix_osdi_ae/serial.out -append "rootdevname=c0d0p0 cttyline=0"`

5. Login as `root` in the MINIX 3 shell.

6. Press `x` and then the `Enter` key in the MINIX 3 shell. The fault corresponding to the currently checked-out branch will be injected into MINIX's IPC channel between input and tty processes.

7. The output will be logged to `serial.out`. For all faults except `s1` and `s6`, the log will show a kernel panic in MINIX followed by restart.

8. Repeat from step 2 to generate other faults.
