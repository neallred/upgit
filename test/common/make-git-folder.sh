#!/bin/bash

upgit_dir=$(dirname "$0")
git_folder="git-folder"


if [ "$1" == "--recreate" ]; then
  echo "recreate flag passed, removing git folder and recreating from scratch"
  rm -rf $upgit_dir/$git_folder
fi

pushd () {
  command pushd "$@" > /dev/null
}

popd () {
  command popd "$@" > /dev/null
}

# Make a non git repo
mkdir -p $upgit_dir/$git_folder/plain-dir

# Make a local repo
mkdir -p $upgit_dir/$git_folder/local-repo
pushd $upgit_dir/$git_folder/local-repo
git init > /dev/null
echo '' > dummy-file
git add dummy-file
git commit -m 'Initial commit' > /dev/null
popd

# Make an empty repo
mkdir -p $upgit_dir/$git_folder/empty-repo
pushd $upgit_dir/$git_folder/empty-repo
git init > /dev/null
git remote add origin https://github.com/neallred/upgit.git
popd

# Make an single remote, non origin repo
mkdir -p $upgit_dir/$git_folder/single-no-origin
pushd $upgit_dir/$git_folder/single-no-origin
git init > /dev/null
git remote add not-origin https://github.com/neallred/upgit.git
popd

# Make a multi-remote, non origin repo
mkdir -p $upgit_dir/$git_folder/multi-no-origin
pushd $upgit_dir/$git_folder/multi-no-origin
git init > /dev/null
git remote add not-origin https://github.com/neallred/upgit.git
git remote add not-origin-2 https://github.com/neallred/upgit.git
popd

# Make an multi-remote, origin repo
mkdir -p $upgit_dir/$git_folder/multi-with-origin
pushd $upgit_dir/$git_folder/multi-with-origin
git init > /dev/null
git remote add origin https://github.com/neallred/upgit.git
git remote add not-origin https://github.com/neallred/upgit.git
popd

# Change a legitimate repo
git clone https://github.com/neallred/upgit.git $upgit_dir/$git_folder/upgit-changed > /dev/null 2> /dev/null
# only change if repo created successfully. Otherwise, will be modifying upgit
pushd $upgit_dir/$git_folder/upgit-changed && truncate -s 0 README.md
popd

# Legitimate, bare repo
git clone --bare https://github.com/neallred/upgit.git $upgit_dir/$git_folder/upgit-bare > /dev/null 2> /dev/null

# Unshared branch on legitimate repo
git clone https://github.com/neallred/upgit.git $upgit_dir/$git_folder/upgit-unshared-branch > /dev/null 2> /dev/null
# only change if repo created successfully. Otherwise, will be modifying upgit
pushd $upgit_dir/$git_folder/upgit-unshared-branch && git checkout -b unshared-branch-testing-upgit > /dev/null 2> /dev/null
popd
