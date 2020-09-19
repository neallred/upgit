#!/usr/bin/env bash

my_path="$HOME/.cargo/bin:$PATH"
PATH="$my_path" cargo build
cp target/debug/upgit test/e2e/upgit-linux
docker build -t upgit-test-pull-image -f ./test/e2e/test-pull.Dockerfile ./test/e2e
docker build -t upgit-git-server -f ./test/e2e/git-server.Dockerfile ./test/e2e
cd test/e2e
docker-compose up --build --force-recreate
