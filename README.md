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

From the repo root, invoke

```
./make-testing-repo-container.sh
```

This will create a skeleton directory with several repositories to facilitate testing. `make-testing-repo-container` takes a `--recreate` option that will remove the skeleton directory completely and start it over from scratch. Some of the repos created depend on network access.

Automated tests are still TODO, but you can manually run `./make-testing-repo-container.sh --recreate && go run cmd/upgit.go ./test-repo-container` and check if the output matches expectations.
