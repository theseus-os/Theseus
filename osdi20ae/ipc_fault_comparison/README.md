# IPC fault comparison between Theseus and MINIX3

This folder contains necessary components to evaluate fault recovery in IPC channels of Theseus and MINIX3

## Description
This folder contains a Theseus iso images. The image is generated with settings:

   `make iso THESEUS_CONFIG="unwind_exceptions"`

The iso image contain a modified rendezvous channel with the listed faults injected successively.

[script.sh](./script.sh) contains the top-level script to run the process. The script runs the iso image on a QEMU emulator. The script analyzes the serial console/log output of the run.

[modified_files](./modified_files/) contains the modified rendezvous channel and top level application.

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

   `ipc_faults -FAULT_ID`

Eg

   `ipc_faults -R 3` to generate fault listed as R3 in the table

## Evaluation Process on MINIX 3

Modified source code for MINIX 3 is available at  [minix_osdi_ae](https://github.com/theseus-os/minix_osdi_ae). The repository contain a separate branch for each fault based on fault id. To reproduce each fault checkout the branch and build MINIX 3 using `bash ./releasetools/x86_hdimage.sh` . 

After booting up login to Minix 3 using `root` as username. The source is modified to inject each fault in the table listed above to the IPC channel between input process and tty process, when letter `x` is entered to the shell. 