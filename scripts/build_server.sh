#!/bin/bash
set -e

### This script is invoked from the Theseus top-level Makefile, using 'make build_server',
### should be run on a machine that you can host a publicly-accessible HTTP server.


### This script requires rhash and python
if ! command -v rhash > /dev/null ; then 
  echo "The 'rhash' program is missing, please install it."
fi
if ! command -v python > /dev/null ; then
  echo "The 'python' program is missing, please install it."
fi


### first argument is required:  directory that is being exposed as the root of the HTTP web server
if [ -z $1 ] ; then 
	echo "Error: expected one argument, the top-level directory accessible to the clients via an HTTP server."
	exit 1
fi
HTTP_ROOT=$(readlink -m $1)

### second argument is required:  directory of where the new modules were just built
if [ -z $2 ] ; then 
	echo "Error: missing second argument: the directory containing all of the newly-built module files"
	exit 1
fi
MODULES_DIR=$(readlink -m $2)

### third argument is optional:  the name of the directory that will contain the new modules
if [ -z $3 ] ; then 
  NEW_DIR_NAME=$(date - u | sed '/s/ /_/g')
	echo "No new directory name given, using the current date instead: $NEW_DIR_NAME"
else 
  NEW_DIR_NAME=$3
fi

### create a directory to hold the newly-built module files and their checksums
NEW_DIR=$(readlink -m $HTTP_ROOT/$NEW_DIR_NAME)
rm -rf $NEW_DIR
mkdir -p $NEW_DIR
cp $MODULES_DIR/*.o $NEW_DIR/

# echo "HTTP_ROOT: $HTTP_ROOT"
# echo "MODULES_DIR: $MODULES_DIR"
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

### Update the root listing to reflect all available update directories.
### The root listing sorts directories in reverse chronological order (newest at top, oldest at bottom),
### without the trailing slash that usually is appended on directory names.
rm -rf $HTTP_ROOT/updates.txt
cd $HTTP_ROOT/
for d in $(ls -dt */ | sed 's/[/]//g') ; do
  echo "$d" >> $HTTP_ROOT/updates.txt
done


### It's okay to invoke the http server without checking to see if it's already running, because if it is, 
### then invoking it again will just error out silently and let the existing instance keep running.
echo "Starting simple HTTP server at root dir $HTTP_ROOT with new dir $NEW_DIR_NAME"
cd $HTTP_ROOT && python -m SimpleHTTPServer 8090  > /dev/null  2>&1  &
