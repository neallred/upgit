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
