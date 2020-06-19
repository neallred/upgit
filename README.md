# upgit

Command line tool for bringing git repos up to date and listing their status. Think `find . -mindepth 1 -maxdepth 1 -type d -exec git -C pull \;`, but parallel.

Inspired by countless minutes lost repeating `cd <some-repo-path> && git pull` for countless repos. Credit to https://gabac.blog/posts/git-pull-many-repos-at-once/ for the shell script one-liner.

## Running

From the repo root, run

```
go run cmd/upgit.go <relative path(s) to repo container folder(s)>
```

For example, if I stored all my repos in ~/github, `go run cmd/upgit.go ~/github`

## Installing

```
go build -o upgit cmd/upgit.go
```

Move upgit executable to somewhere on your `$PATH`.

## Testing

### Unit

TBD

### Integration

From the repo root, run

```
go build -o test/common cmd/upgit.go
./test/common/make-git-folder.sh
./test/common/upgit ./test/common/git-folder
```

`make-git-folder.sh` create several repos as test/common/git-folder. It accepts a `--recreate` option that will remove the skeleton directory completely and start it over from scratch. Some of the repos created depend on network access.

Automated assertions are still TODO, but you can manually check if STDOUT matches expectations.

### End to end
From the repo root, run

```
GOOS=linux go build -o test/common/upgit-linux cmd/upgit.go
docker build -t upgit-test-pull-image -f ./test/e2e/test-pull.Dockerfile ./test/e2e
docker build -t upgit-git-server -f ./test/e2e/git-server.Dockerfile ./test/e2e
cd test/e2e
docker-compose up --build --force-recreate
```

The end to end tests use docker-compose to build a common, reused git server, and various git clients. Each git client is an end to end test.

Pulling and reporting updates to one repo. The git client test script:
  * makes a repo
  * pushes it to the git server
  * clones it to another folder
  * updates the original repo and the git server origin repo
  * runs upgit, checking that the clone is updated
