#!/bin/bash

upgit_dir=$(dirname "$0")


if [ "$1" == "--recreate" ]; then
  echo "recreate flag passed, removing test-repo-container and recreating from scratch"
  rm -rf $upgit_dir/test-repo-container
fi

pushd () {
  command pushd "$@" > /dev/null
}

popd () {
  command popd "$@" > /dev/null
}

# Make a non git repo
mkdir -p $upgit_dir/test-repo-container/plain-dir

# Make a local repo
mkdir -p $upgit_dir/test-repo-container/local-repo
pushd $upgit_dir/test-repo-container/local-repo
git init > /dev/null
popd

# Make an empty repo
mkdir -p $upgit_dir/test-repo-container/empty-repo
pushd $upgit_dir/test-repo-container/empty-repo
git init > /dev/null
git remote add origin https://github.com/neallred/upgit.git
popd

# Make an single remote, non origin repo
mkdir -p $upgit_dir/test-repo-container/single-no-origin
pushd $upgit_dir/test-repo-container/single-no-origin
git init > /dev/null
git remote add not-origin https://github.com/neallred/upgit.git
popd

# Make a multi-remote, non origin repo
mkdir -p $upgit_dir/test-repo-container/multi-no-origin
pushd $upgit_dir/test-repo-container/multi-no-origin
git init > /dev/null
git remote add not-origin https://github.com/neallred/upgit.git
git remote add not-origin-2 https://github.com/neallred/upgit.git
popd

# Make an multi-remote, origin repo
mkdir -p $upgit_dir/test-repo-container/multi-with-origin
pushd $upgit_dir/test-repo-container/multi-with-origin
git init > /dev/null
git remote add origin https://github.com/neallred/upgit.git
git remote add not-origin https://github.com/neallred/upgit.git
popd

# Change a legitimate repo
git clone https://github.com/neallred/upgit.git $upgit_dir/test-repo-container/upgit-changed > /dev/null 2> /dev/null
# only truncate if repo creation successful. Otherwise, will be truncating the repo's readme
pushd $upgit_dir/test-repo-container/upgit-changed && truncate -s 0 README.md
popd

# Unshared branch on legitimate repo
git clone https://github.com/neallred/upgit.git $upgit_dir/test-repo-container/upgit-unshared-branch > /dev/null 2> /dev/null
pushd $upgit_dir/test-repo-container/upgit-unshared-branch
git checkout -b unshared-branch-testing-upgit > /dev/null
popd
