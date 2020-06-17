package main

import (
	"flag"
	"fmt"
	"io/ioutil"
	"log"
	"os"

	"github.com/cheggaaa/pb/v3"
	"github.com/go-git/go-git/v5"
)

type Upgit struct {
	RepoPath          string
	IsRepo            bool
	MatchesRemoteHead string
	Dirty             bool
	UpdateErr         error
}

func QuitOnErr(err error, extra_messages ...string) {
	if err != nil {
		for _, m := range extra_messages {
			log.Println(m)
		}
		log.Fatal(err)
	}
}

func pullRepo(repoStrPath string, ch chan<- Upgit) {
	fmt.Println(repoStrPath)
	upgit := Upgit{
		RepoPath:          repoStrPath,
		IsRepo:            true,
		MatchesRemoteHead: "",
		Dirty:             false,
		UpdateErr:         nil,
	}

	r, err := git.PlainOpen(repoStrPath)
	if err == git.ErrRepositoryNotExists {
		upgit.IsRepo = false
		ch <- upgit
		return
	} else {
		QuitOnErr(err)
	}
	w, err := r.Worktree()
	QuitOnErr(err, "worktree err")
	// TODO: when no remote exists, its not erroring.
	// How to check that remote exists?
	// How to handle if it does not exist?
	pullErr := w.Pull(&git.PullOptions{RemoteName: "origin"})
	if pullErr == git.NoErrAlreadyUpToDate {
	}
	QuitOnErr(err, "pull err")
	ref, err := r.Head()
	QuitOnErr(err, "Head err")
	_, err = r.CommitObject(ref.Hash())
	QuitOnErr(err, "commit err")

	status, err := w.Status()
	QuitOnErr(err, "status err")
	isClean := status.IsClean()
	if !isClean {
		fmt.Println("repo is dirty:", repoStrPath)
		upgit.Dirty = true
		ch <- upgit
		return
	}

	ch <- upgit
	return
}

func min(x, y int) int {
	if x < y {
		return x
	}
	return y
}

// don't update too many repos at once, don't want to exceed open file limit
const MAX_CONCURRENT_UPGITS = 20

func main() {
	flag.Parse()
	repoContainers := os.Args[1:]
	if len(repoContainers) == 0 {
		// TODO: allow reading from environment variables, configs, flags,
		// and prompting user for container paths
		log.Fatal("Please pass paths to project containers as arguments")
	}
	fmt.Println("project_containers", repoContainers)
	repoPaths := []string{}
	for _, repoContainer := range repoContainers {
		files, err := ioutil.ReadDir(repoContainer)
		QuitOnErr(err)
		for _, repo := range files {
			if repo.IsDir() {
				repoPaths = append(repoPaths, fmt.Sprintf("%s/%s", repoContainer, repo.Name()))
			}
		}
	}

	lenRepoPaths := len(repoPaths)
	chUpgit := make(chan Upgit, min(lenRepoPaths, MAX_CONCURRENT_UPGITS))
	defer close(chUpgit)

	fmt.Printf("upgitting %d repos\n", lenRepoPaths)
	bar := pb.New(lenRepoPaths).SetWidth(80)
	bar.Start()

	for _, repoPath := range repoPaths {
		go pullRepo(repoPath, chUpgit)
	}

	results := []Upgit{}
	for i := 0; i < lenRepoPaths; i++ {
		upgit := <-chUpgit
		results = append(results, upgit)
		bar.Increment()
	}
	bar.Finish()
}
