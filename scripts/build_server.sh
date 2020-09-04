#!/bin/bash
set -e

### This script is invoked from the Theseus top-level Makefile, using 'make build_server',
### should be run on a machine that you can host a publicly-accessible HTTP server.


### the directory containing this script 
SCRIPTS_DIR="$(dirname "$(readlink -f "$0")")"
THESEUS_BASE_DIR=$SCRIPTS_DIR/..
TOOLS_DIR=$THESEUS_BASE_DIR/tools

### This script requires rhash and python
if ! command -v rhash > /dev/null ; then 
  echo "The 'rhash' program is missing, please install it."
fi
if ! command -v python > /dev/null ; then
  echo "The 'python' program is missing, please install it."
fi


### required argument:  directory of where the new modules were just built
if [ -z $NEW_MODULES_DIR ] ; then 
	echo "Error: missing NEW_MODULES_DIR var: the directory containing all of the newly-built module files"
	exit 1
fi
NEW_MODULES_DIR=$(readlink -m $NEW_MODULES_DIR)

### optional argument:  directory that is being exposed as the root of the HTTP web server
if [ -z $HTTP_ROOT ] ; then 
  HTTP_ROOT=$THESEUS_BASE_DIR/.theseus_build_server
	echo "No HTTP_ROOT directory given, using the default directory \"$HTTP_ROOT\""
fi
HTTP_ROOT=$(readlink -m $HTTP_ROOT)

### optional argument:  the name of the directory that will contain the new modules
if [ -z $NEW_DIR_NAME ] ; then 
  NEW_DIR_NAME=$(date - u | sed '/s/ /_/g')
	echo "No new directory name given, using the current date instead: $NEW_DIR_NAME"
fi


### create a directory to hold the newly-built module files and their checksums
NEW_DIR=$(readlink -m $HTTP_ROOT/$NEW_DIR_NAME)
rm -rf $NEW_DIR
mkdir -p $NEW_DIR
cp $NEW_MODULES_DIR/*.o $NEW_DIR/

# echo "HTTP_ROOT: $HTTP_ROOT"
# echo "NEW_MODULES_DIR: $NEW_MODULES_DIR"
# echo "NEW_DIR_NAME: $NEW_DIR_NAME"
# echo "NEW_DIR: $NEW_DIR"


### calculate the checksums for each of the new module files
mkdir -p $NEW_DIR/checksums
cd $NEW_DIR/
for f in *.o ; do
  rhash --sha3-512 $f -o $NEW_DIR/checksums/$(basename $f).sha512
done

### create a simple listing of all module files
cd $NEW_DIR/
ls *.o > $NEW_DIR/listing.txt


### If the directory of old modules was optionally provided, create a diff file in the new update dir.
### If a state transfer function was specified, then append it to the end of the diff
if [ -d $OLD_MODULES_DIR ] ; then 
  # DIFF_FILE=$(readlink -e $DIFF_FILE)
	cargo run --manifest-path $TOOLS_DIR/diff_crates/Cargo.toml -- $OLD_MODULES_DIR  $NEW_MODULES_DIR  >  $NEW_DIR/diff.txt
if [ ! -z $STATE_TRANSFER ] ; then
  echo "@$STATE_TRANSFER" >>  $NEW_DIR/diff.txt
fi
fi


### Update the root listing to reflect all available update directories.
### The root listing sorts directories in reverse chronological order (newest at top, oldest at bottom),
### without the trailing slash that usually is appended on directory names.
rm -rf $HTTP_ROOT/updates.txt
cd $HTTP_ROOT/
for d in $(ls -dt */ | sed 's/[/]//g') ; do
  echo "$d" >> $HTTP_ROOT/updates.txt
done


### Start up the actual HTTP server (after killing off any existing identical web server)
WEBSERVER_CMD="python -m SimpleHTTPServer 8090"
pkill -f "$WEBSERVER_CMD" || true
echo "Starting simple HTTP server at \"$HTTP_ROOT\" with new update \"$NEW_DIR_NAME\""
cd $HTTP_ROOT && $($WEBSERVER_CMD) > /dev/null  2>&1  &
