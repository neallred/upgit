#!/bin/bash
t_label="Upgit updates repos that need it"

red() {
  red_color='\033[0;31m'
  no_color='\033[0m'
  echo -e "${red_color}$@${no_color}"
}

green() {
  green_color='\033[0;32m'
  no_color='\033[0m'
  echo -e "${green_color}$@${no_color}"
}

t_fail() {
  red "$1 failed. Output can be examined by execing into the container"
}

t_pass() {
  green "$1"
}

loop_sleep() {
  # keep container up so it can be exec'd into and manually tested/inspected
  sleep 10
  loop_sleep
}

# TODO: This doesn't seem to be taking effect
setup_ssh() {
  echo "Bootstrapping ssh"
  eval "$(ssh-agent)" > /dev/null
  [ -z "$SSH_AUTH_SOCK" ] && SSH_AUTH_SOCK=$(ls -l /tmp/ssh-*/agent.* 2> /dev/null | grep $(whoami) | awk '{print $9}')
  export SSH_AUTH_SOCK=$SSH_AUTH_SOCK
  ssh_add_output=$(ssh-add /root/.ssh/id_rsa 2>&1 1> /dev/null)
}
setup_ssh

ensure_git() {
  git config --global user.email "upgit@git_client.com"
  git config --global user.name "upgit"

  git_space="$(pwd)/git_space"
  repo_a="$git_space/dummy-repo-a"
  repo_b="$git_space/dummy-repo-b"
}
ensure_git

push_a() {
  echo Repo a: initial add and push
  mkdir -p $repo_a
  cd $repo_a
  git init --quiet
  echo '#Hello World' > README.md
  git add README.md
  git commit --quiet -m 'Initial Commit'
  git remote add origin git@git_server:/var/git/dummy-repo.git 2>&1 1> /dev/null
  push_output=$(git push --quiet origin -u master 2>&1 1> /dev/null)
}
push_a

clone_b() {
  echo Repo b: clone initial commit
  cd $git_space
  git clone --quiet git@git_server:/var/git/dummy-repo.git dummy-repo-b
}
clone_b

update_a() {
  second_commit_message="Second commit. Run instructions"
  echo Repo a: subsequent commit and push
  cd $repo_a
  echo '##Running dummy repo' >> README.md
  git add README.md
  git commit --quiet -m "$second_commit_message"
  git push --quiet
}
update_a

upgit_all() {
  echo Test that upgit updates repo b
  cd $git_space
  upgit_stdout=$(/bin/upgit ./)
}
upgit_all

assert() {
  expected_up_to_date='Up to date (1)'
  expected_updated='Updated (1)'
  expected_updated_b='dummy-repo-b'

  echo "$upgit_stdout" | grep --quiet -o "$expected_up_to_date" && \
    echo "$upgit_stdout" | grep --quiet -o "$expected_updated" && \
    echo "$upgit_stdout" | grep --quiet -o "$expected_updated_b" && \
    t_pass "$t_label" && \
    exit 0

  t_fail "$t_label"

  loop_sleep
}
assert
