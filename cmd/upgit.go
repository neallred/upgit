package main

import (
	"flag"
	"fmt"
	"io/ioutil"
	"log"
	"os"
	"regexp"

	"github.com/cheggaaa/pb/v3"
	"github.com/go-git/go-git/v5"
)

type Upgit struct {
	Result UpgitResult
	Path   string
	Report string
}

func mkUpgit(repoPath string) func(UpgitResult, string) Upgit {
	return func(result UpgitResult, report string) Upgit {
		return Upgit{
			Path:   repoPath,
			Result: result,
			Report: report,
		}
	}
}

func QuitOnErr(err error, extra_messages ...string) {
	if err != nil {
		for _, m := range extra_messages {
			log.Println(m)
		}
		log.Fatal(err)
	}
}

type UpgitResult int

const (
	NotARepo UpgitResult = iota
	NoRemotes
	Dirty
	RemoteHeadMismatch
	UpToDate
	Updated
	NoClearOrigin
	BareRepository
	Other // For unconsidered errors.
)

func pullRepo(repoStrPath string, ch chan<- Upgit) {
	toUpgit := mkUpgit(repoStrPath)

	repo, err := git.PlainOpen(repoStrPath)
	if err == git.ErrRepositoryNotExists {
		ch <- toUpgit(NotARepo, "")
		return
	} else {
		QuitOnErr(err)
	}

	remotes, err := repo.Remotes()

	QuitOnErr(err)

	originName := "origin"

	if numRemotes := len(remotes); numRemotes == 0 {
		ch <- toUpgit(NoRemotes, "")
	} else if numRemotes > 1 {
		_, err := repo.Remote("origin")
		if err != nil {
			// TODO: string method is of the form
			// "origin      https://github.com/neallred/rivendell.git (fetch)"
			// so need to parse out the origin names.
			ch <- toUpgit(NoClearOrigin, "")
			return
		}
	} else {
		_, err := repo.Remote("origin")
		if err != nil {
			re := regexp.MustCompile(`^[A-Za-z0-9_-]+`)
			originNameBytes := re.Find([]byte(remotes[0].String()))
			if originNameBytes == nil {
				ch <- toUpgit(NoClearOrigin, "")
				return
			} else {
				originName := string(originNameBytes)
				fmt.Println("hopefully this is a remote name ...", originName)
			}
		}
	}

	w, err := repo.Worktree()
	if err == git.ErrIsBareRepository {
		ch <- toUpgit(BareRepository, "")
		return
	} else {
		QuitOnErr(err, "worktree err")
	}

	status, err := w.Status()
	QuitOnErr(err, "status err")
	isClean := status.IsClean()
	if !isClean {
		fmt.Println("repo is dirty:", repoStrPath)

		submodules, err := w.Submodules()
		QuitOnErr(err)
		for _, s := range submodules {
			fmt.Println("submodule: ", s)
		}

		changedFileReport := ""
		upgit := toUpgit(Dirty, changedFileReport)

		ch <- upgit
		return
	}

	pullErr := w.Pull(&git.PullOptions{RemoteName: originName})
	if pullErr == git.NoErrAlreadyUpToDate {
		ch <- toUpgit(UpToDate, "")
		return
	} else if pullErr == nil {
		// TODO: report of what files changed here
		ch <- toUpgit(Updated, "TODO: report of what files changed here")
		return
	} else {
		QuitOnErr(err, "pull err")
	}

	ref, err := repo.Head()
	if err != nil {
		fmt.Println(err, "Head err", repoStrPath)
	} else {
		_, err = repo.CommitObject(ref.Hash())
		if err != nil {
			fmt.Println("commit err", repoStrPath)
		}
	}

	ch <- toUpgit(Other, "dunno what happened, this state should be unreachable")
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

func listPath(upgit Upgit) {
	fmt.Printf("  %s\n", upgit.Path)
}

func printUpgits(upgits map[UpgitResult][]Upgit) {
	// TODO: Still need to handle: Dirty, RemoteHeadMismatch, Updated,

	if numRepos := len(upgits[NotARepo]); numRepos > 0 {
		fmt.Printf("Not a repo (%d):\n", numRepos)
		for _, upgit := range upgits[NotARepo] {
			listPath(upgit)
		}
	}

	if numRepos := len(upgits[UpToDate]); numRepos > 0 {
		fmt.Printf("%d up to date repos\n", numRepos)
	}

	if numRepos := len(upgits[NoRemotes]); numRepos > 0 {
		fmt.Printf("%d local-only repos with nothing to upgit\n", numRepos)
	}

	if numRepos := len(upgits[BareRepository]); numRepos > 0 {
		fmt.Printf("%d bare repos that do not have work trees\n", numRepos)
	}

	if numRepos := len(upgits[NoClearOrigin]); numRepos > 0 {
		fmt.Printf("Multiple remotes, but no \"origin\". Unable to upgit %d repos:\n", numRepos)

		for _, upgit := range upgits[NoClearOrigin] {
			listPath(upgit)
		}
	}

	if numRepos := len(upgits[Other]); numRepos > 0 {
		fmt.Printf("Repos with unknown outcome (%d) (This should never happen and is probably a logic error in upgit!):\n", numRepos)
		for _, upgit := range upgits[Other] {
			fmt.Printf("  %s\n", upgit.Path)
		}
	}

}

func main() {
	flag.Parse()
	repoContainers := os.Args[1:]
	if len(repoContainers) == 0 {
		// TODO: allow reading from environment variables, configs, flags,
		// and prompting user for container paths
		log.Fatal("Please pass paths to project containers as arguments")
	}
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

	upgits := map[UpgitResult][]Upgit{}
	for i := 0; i < lenRepoPaths; i++ {
		upgit := <-chUpgit
		upgits[upgit.Result] = append(upgits[upgit.Result], upgit)
		bar.Increment()
	}
	bar.Finish()
	printUpgits(upgits)
}
