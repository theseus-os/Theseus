### This dockerfile sets up an Ubuntu 18.04 environment from scratch 
### that is sufficient to build Theseus and run it using QEMU.


FROM ubuntu:18.04
LABEL author=kevinaboos@gmail.com


# These build args are required.
# USER is the current user's name: "kevin"
# UID is that same user's id: "1000"
# GID is that same user's group id: "1000"
ARG USER
ARG UID
ARG GID
RUN test -n "$USER" || (echo "\nError: USER build arg not set\n" && false)
RUN test -n "$UID" || (echo "\nError: UID build arg not set\n" && false)
RUN test -n "$GID" || (echo "\nError: GID build arg not set\n" && false)


# Basic set up for ubuntu image
# Remove some warnings about deb pkg manager
# See the following links:
# * https://github.com/phusion/baseimage-docker/issues/58
# * https://github.com/phusion/baseimage-docker/issues/319#issuecomment-245857919
ENV DEBIAN_FRONTEND noninteractive
RUN DEBIAN_FRONTEND=noninteractive apt-get update && apt-get install -y --no-install-recommends apt-utils gnupg

# Install essential Ubuntu packages
RUN apt-get install -y build-essential curl git

# Install Theseus's build dependencies
RUN apt-get install -y make nasm pkg-config grub-pc-bin mtools xorriso
# Install QEMU and KVM, based on <https://help.ubuntu.com/community/KVM/Installation>
RUN apt-get install -y qemu qemu-kvm libvirt-daemon-system libvirt-clients bridge-utils

# (Optional) Install packages for convenience purposes
RUN apt-get install -y vim gdb

# (Optional) Install packages used for running our 
RUN apt-get install -y python2.7 rhash python3.6 python3-pip xterm
RUN pip3 install PTable

# Undo the noninteractive DEBIAN_FRONTEND from the beginning of this file
ENV DEBIAN_FRONTEND teletype


# Create a user (and a group of the same name) that matches the current host user.
RUN groupadd -o -g ${GID} ${USER}
RUN useradd -m -s /bin/bash -u ${UID} -g ${GID} ${USER}
# All commands below here will now run as the new user.
# However, we must manually set the $HOME variable, since the above `USER` command only applies to RUN, not ENV commands.
USER ${USER}
ENV HOME="/home/${USER}"

# Install Rust via the online instructions: <https://www.rust-lang.org/tools/install>.
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y
# To enable us to use Rust, add its binaries to our path.
ENV PATH="$HOME/.cargo/bin:${PATH}"
