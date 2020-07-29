#!/bin/bash

loop_sleep() {
  # keep container up so it can be exec'd into and manually tested/inspected
  sleep 10
  loop_sleep
}

ssh-agent
[ -z "$SSH_AUTH_SOCK" ] && SSH_AUTH_SOCK=$(ls -l /tmp/ssh-*/agent.* 2> /dev/null | grep $(whoami) | awk '{print $9}')
export SSH_AUTH_SOCK=$SSH_AUTH_SOCK
ssh-add /root/.ssh/id_rsa

git config --global user.email "upgit@git_client.com"
git config --global user.name "upgit"

git_space="$(pwd)/git_space"
repo_a="$(pwd)/git_space/dummy-repo-a"
repo_b="$(pwd)/git_space/dummy-repo-a"

echo Repo a: initial add and push
mkdir -p $repo_a
cd $repo_a
git init
echo '#Hello World' > README.md
git add README.md
git commit -m 'Initial Commit'
git remote add origin git@git_server:/var/git/dummy-repo.git
git push origin -u master

echo Repo b: clone initial commit
cd $git_space
git clone git@git_server:/var/git/dummy-repo.git dummy-repo-b

second_commit_message="Second commit. Run instructions"
echo Repo a: subsequent commit and push
cd $repo_a
echo '##Running dummy repo' >> README.md
git add README.md
git commit -m "$second_commit_message"
git push

echo Test that upgit updates repo b
cd $git_space
/bin/upgit ./

loop_sleep
