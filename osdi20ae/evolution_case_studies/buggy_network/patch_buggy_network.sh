#!/bin/bash
set -e 

### This script restores the changes from a version of Theseus that had buggy network behavior
### such that we can demonstrate evolution from that buggy version to the now-working version.

### the directory containing this script 
BUGGY_NETWORK_DIR="$(dirname "$(readlink -f "$0")")"
THESEUS_BASE_DIR=$BUGGY_NETWORK_DIR/../../..

### The fixed commit contains multiple files that were modified. Copy them over to the right place. 
cp -vf $BUGGY_NETWORK_DIR/e1000.rs                       $THESEUS_BASE_DIR/kernel/e1000/src/lib.rs
cp -vf $BUGGY_NETWORK_DIR/intel_ethernet_descriptors.rs  $THESEUS_BASE_DIR/kernel/intel_ethernet/src/descriptors.rs
cp -vf $BUGGY_NETWORK_DIR/ota_update_client.rs           $THESEUS_BASE_DIR/kernel/ota_update_client/src/lib.rs
cp -vf $BUGGY_NETWORK_DIR/nic_initialization.rs          $THESEUS_BASE_DIR/kernel/nic_initialization/src/lib.rs

echo "Patch applied successfully."
