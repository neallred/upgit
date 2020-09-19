#!/usr/bin/env bash

my_path="$HOME/.bin/cargo:$PATH"
PATH="$my_path" cargo build

# `make-git-folder.sh` creates several repos as test/common/git-folder.
# It accepts a `--recreate` option that will remove the skeleton directory completely
# and start it over from scratch.
# Some of the repos created depend on network access.

./test/common/make-git-folder.sh --recreate
./target/debug/upgit ./test/common/git-folder
