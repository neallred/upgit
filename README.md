# upgit

Command line tool for bringing git repos up to date, quickly. Uses the tokio runtime to pull multiple repos at once. Inspired by countless repetitions of "Do you have latest master?" and `cd <some-repo-path> && git pull`.

## Prerequisites

A working [`cargo`](https://doc.rust-lang.org/cargo) install. It (and `rustup`) can be installed by following the instructions on [rustup.rs](https://rustup.rs).

## Usage

Run `upgit --help` for the following usage help.

```
upgit 0.1.0
Nathaniel Allred <neallred@gmail.com>
Pulls all repos within a folder containing git projects, in parallel. Supports
configuration via command line flags and params, or via ENV vars. Command line
takes precedence. If no option is set but is needed (i.e. repos requiring auth),
user will be prompted if a TTY is available, or skip that repo if it is not
available.

USAGE:
    upgit [FLAGS] [OPTIONS] [--] [git-dirs]...

FLAGS:
        --default-plain
            Default password to attempt for https cloned repos. User will be
            prompted for the password. Env var is UPGIT_DEFAULT_PLAIN set to any
            value.
        --default-ssh
            Default password to use for ssh keys. User will be prompted for the
            password. Env var is UPGIT_DEFAULT_SSH set to any value.
    -h, --help
            Prints help information

    -V, --version
            Prints version information


OPTIONS:
        --plain <plain>...
            Git repo https url with username. For example, `--plain
            https://neallred@bitbucket.org/neallred/allredlib-data-backup.git`.
            For each time this option is passed, user will be prompted for a
            password. Env var is comma separated UPGIT_PLAIN.
        --share <share>
            Degree to which credentials may reused between repos needing auth.
            Each level is additive. `none` means no credential reuse between
            repos, and defaults are ignored. `default` means default provided
            credentials may be reused. `duplicate` means deefaults, plus
            multiple copies of a repo can reuse each other's credential. `org`
            means duplicate, plus upgit will infer a matching org by looking at
            the second to last url path segment (e.g. `neallred` in
            https://github.com/neallred/upgit`). `domain` means reusing when
            user and url domain match. Env var is UPGIT_SHARE. [default:
            default]  [possible values: none, default, duplicate, org, domain]
        --ssh <ssh>...
            Paths to ssh keys to preverify. User will be prompted for password
            for each key given. Can enter "blank" if ssh key is not password
            protected. Env var is comma separated UPGIT_SSH.

ARGS:
    <git-dirs>...
            Paths (relative or absolute) to folders that contain git repos. Env
            var is comma separated UPGIT_GIT_DIRS.
```

Examples

Update all the repos contained in the `github` folder, and all the repos contained in the `bitbucket` folder:

```
upgit ~/github ~/bitbucket
```

Supposing you work for a company in which you are part of multiple teams, and you organize your repos according to the teams you are on, you can update them all like this:

```
upgit ~/megacorp/team-a ~/megacorp/team-b
```

Update all repos in the `github` folder, being prompted immediately for the password to an assumed ssh key in `$HOME/.ssh/id_rsa`:

```
upgit --default-ssh ~/github
```

A way of only entering a password once per domain when all your orgs/repos passwords for a given user are the same:

```
upgit --share domain --plain https://neallred@bitbucket.org/ --plain https://neallred@github.com/ ~/github
```

### Authentication

`upgit` supports ssh and user/pass auth methods. It is work in progress and subject to change. The user is prompted when needed, or when the user signals preprovision of credentials via flags or env vars.

Ssh supports multiple keys. If none is provided, and a repo requests ssh authentication, upgit assumes the private key is at `$HOME/.ssh/id_rsa`. It allows entering a blank ssh passphrase if there is no passphrase on the key.

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
./test/common/make-git-folder.sh --recreate
./target/debug/upgit ./test/common/git-folder
```

`make-git-folder.sh` creates several repos as test/common/git-folder. It accepts a `--recreate` option that will remove the skeleton directory completely and start it over from scratch. Some of the repos created depend on network access.

Automated assertions are still TODO, but you can manually check if STDOUT matches expectations.

### End to end
From the repo root, run

```
cargo build
cp target/debug/upgit test/e2e/upgit-linux
docker build -t upgit-test-pull-image -f ./test/e2e/test-pull.Dockerfile ./test/e2e
docker build -t upgit-git-server -f ./test/e2e/git-server.Dockerfile ./test/e2e
cd test/e2e
docker-compose up --build --force-recreate
```

The end to end tests use docker-compose to build a common, reused git server, and various git clients. Each git client is an end to end test. Current tests are :

1. Pulling and reporting updates to one repo. The test script:
  * makes a repo
  * pushes it to the git server
  * clones it to another folder
  * updates the original repo and the git server origin repo
  * runs upgit, checking that the clone is updated

