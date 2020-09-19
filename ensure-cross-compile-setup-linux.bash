#!/usr/bin/env bash
sudo apt update
sudo apt install clang gcc g++ zlib1-dev libmpc-dev libmpfr-dev libgmp-dev libxml2-dev wget
rustup target add x86_64-apple-darwin
git submodule update --init --recursive
cd osxcross
wget -nc https://s3.dockerproject.org/darwin/v2/MacOSX10.10.sdk.tar.xz
mv MacOSX10.10.sdk.tar.xz tarballs/
UNATTENDED=yes OSX_VERSION_MIN=10.7 ./build.sh
cd ..
