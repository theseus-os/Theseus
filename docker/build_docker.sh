#!/bin/bash

### This script creates a docker image (based on the Dockerfile )
### that is capable of building and running Theseus. 
### To run the docker image as a container on your local machine,
### use the `./docker/run_docker.sh` script.

set -e

DOCKER_TAG="theseus:Dockerfile"

# DOCKER_DIR is the directory containing this docker script and the Dockerfile
DOCKER_DIR=$(dirname $(readlink -f ${BASH_SOURCE}))
### THESEUS_BASE_DIR is the base directory of the Theseus repository.
THESEUS_BASE_DIR=$(readlink -f ${DOCKER_DIR}/.. )

### Always run this script with the `docker` directory as the working directory.
cd ${DOCKER_DIR} 

### Build the docker image
docker build \
    --build-arg USER=$(id -un) \
    --build-arg UID=$(id -u) \
    --build-arg GID=$(id -g) \
    -t ${DOCKER_TAG}  ./

echo -e "$(tput setaf 10)\nDocker image built successfully. Next, run it as a local container:$(tput sgr0)"
echo "    ./docker/run_docker.sh"
