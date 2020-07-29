# upgit

Command line tool for bringing git repos up to date, quickly. Uses the tokio runtime to pull multiple repos at once. Inspired by countless repetitions of "Do you have latest master?" and `cd <some-repo-path> && git pull`.

## Prerequisites

A working [`cargo`](https://doc.rust-lang.org/cargo) install. It (and `rustup`) can be installed by following the instructions on [rustup.rs](https://rustup.rs).

## Usage

### Authentication

Currently, `upgit` supports basic credential and ssh auth methods. It is work in progress and will likely change significantly. The user is not prompted for auth unless the specific repo requests it, or unless they signal they want to preprovide credentials.

The ssh method naively assumes the private key is at $HOME/.ssh/id_rsa. It allows entering a blank ssh passphrase if there is no passphrase on the key. It accepts the password and does not allow different keys to be used per repo.

The plain text method assumes the last password entered is the one that should be used for unseen URLs. Because of how threading is currently implemented, entering a wrong password means a LOT of password re-entry. To mitigate this, plain text password entry prompts for password confirmation.


## Running

From the repo root, run

```
cargo run <relative path(s) to folder holding git project(s)>
```

For example, if I stored all my repos in `~/github`, `cargo run ~/github`

## Building

From the repo root, run

```
cargo build --release
```

Move the `target/release/upgit` executable to somewhere on your `$PATH`.

## Testing

### Unit

Run `cargo test`.

### Integration

From the repo root, run

```
cargo build
cp target/debug/upgit test/common/upgit
./test/common/make-git-folder.sh
./test/common/upgit ./test/common/git-folder
```

`make-git-folder.sh` create several repos as test/common/git-folder. It accepts a `--recreate` option that will remove the skeleton directory completely and start it over from scratch. Some of the repos created depend on network access.

Automated assertions are still TODO, but you can manually check if STDOUT matches expectations.

### End to end
From the repo root, run

```
cargo build
cp target/debug/upgit test/common/upgit-linux
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
