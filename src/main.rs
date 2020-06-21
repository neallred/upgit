use git2::Repository;
use std::io::{self, Write};
use std::str;
use std::fs;
use std::env;

fn do_fetch<'a>(
    repo: &'a git2::Repository,
    refs: &[&str],
    remote: &'a mut git2::Remote,
    repo_path: &String,
) -> Result<git2::AnnotatedCommit<'a>, git2::Error> {
    // let mut cb = git2::RemoteCallbacks::new();

    // Print out our transfer progress
    // cb.transfer_progress(|stats| {
    //     if stats.received_objects() == stats.total_objects() {
    //         print!(
    //             "Resolving deltas{}/{}\r",
    //             stats.indexed_deltas(),
    //             stats.total_deltas()
    //         );
    //     } else if stats.total_objects() > 0 {
    //         print!(
    //             "Received {}/{} objects ({}) in {} bytes\r",
    //             stats.received_objects(),
    //             stats.total_objects(),
    //             stats.indexed_objects(),
    //             stats.received_bytes(),
    //         );
    //     }
    //     io::stdout().flush().unwrap();
    //     true
    // });

    let mut fo = git2::FetchOptions::new();
    // fo.remote_callbacks(cb);
    // Always fetch all tags.
    // Perform a download and also update tips
    fo.download_tags(git2::AutotagOption::All);
    println!("Fetching {} for {}", remote.name().unwrap(), repo_path);
    remote.fetch(refs, Some(&mut fo), None)?;

    // If there are local objects (we got a thin pack), then tell the user
    // how many objects we saved from having to cross the network.
    let stats = remote.stats();
    if stats.local_objects() > 0 && stats.received_bytes() > 0 {
        println!(
            "\rReceived {}/{} objects in {} bytes (used {} local \
            objects)",
            stats.received_objects(),
            stats.total_objects(),
            stats.indexed_objects(),
            stats.received_bytes(),
        );
    }

    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    Ok(repo.reference_to_annotated_commit(&fetch_head)?)
}

fn fast_forward(
    repo: &Repository,
    lb: &mut git2::Reference,
    rc: &git2::AnnotatedCommit,
) -> Result<(), git2::Error> {
    let name = match lb.name() {
        Some(s) => s.to_string(),
        None => String::from_utf8_lossy(lb.name_bytes()).to_string(),
    };
    let msg = format!("Fast-Forward: Setting {} to id: {}", name, rc.id());
    println!("{}", msg);
    lb.set_target(rc.id(), &msg)?;
    repo.set_head(&name)?;
    repo.checkout_head(Some(
        git2::build::CheckoutBuilder::default()
            // force required to make the working directory actually get updated/ 
            // could add logic to handle dirty working diredtory states
            .force(),
    ))?;
    Ok(())
}

fn normal_merge(
    repo: &Repository,
    local: &git2::AnnotatedCommit,
    remote: &git2::AnnotatedCommit,
) -> Result<(), git2::Error> {
    let local_tree = repo.find_commit(local.id())?.tree()?;
    let remote_tree = repo.find_commit(remote.id())?.tree()?;
    let ancestor = repo
        .find_commit(repo.merge_base(local.id(), remote.id())?)?
        .tree()?;
    let mut idx = repo.merge_trees(&ancestor, &local_tree, &remote_tree, None)?;

    if idx.has_conflicts() {
        println!("Merge conflicts detected...");
        repo.checkout_index(Some(&mut idx), None)?;
        return Ok(());
    }
    let result_tree = repo.find_tree(idx.write_tree_to(repo)?)?;
    // now create the merge commit
    let msg = format!("Merge: {} into {}", remote.id(), local.id());
    let sig = repo.signature()?;
    let local_commit = repo.find_commit(local.id())?;
    let remote_commit = repo.find_commit(remote.id())?;
    // do our merge commit and set current branch head to that commit.
    let _merge_commit = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &msg,
        &result_tree,
        &[&local_commit, &remote_commit],
    )?;
    // Set working tree to match head.
    repo.checkout_head(None)?;
    Ok(())
}

fn do_merge<'a>(
    repo: &'a Repository,
    remote_branch: &str,
    fetch_commit: git2::AnnotatedCommit<'a>,
) -> Result<(), git2::Error> {
    // 1. do a merge analysis
    let analysis = repo.merge_analysis(&[&fetch_commit])?;

    // 2. Do the appropriate merge
    if analysis.0.is_fast_forward() {
        println!("Doing a fast forward");
        // do a fast forward
        let refname = format!("refs/heads/{}", remote_branch);
        match repo.find_reference(&refname) {
            Ok(mut r) => {
                fast_forward(repo, &mut r, &fetch_commit)?;
            }
            Err(_) => {
                // The branch doesn't exist so just set the reference to the
                // commit directly. Usually this is because you are
                // pulling into an empty repository.
                repo.reference(
                    &refname,
                    fetch_commit.id(),
                    true,
                    &format!("Setting {} to {}", remote_branch, fetch_commit.id()),
                )?;
                repo.set_head(&refname)?;
                repo.checkout_head(Some(
                        git2::build::CheckoutBuilder::default()
                            .allow_conflicts(true)
                            .conflict_style_merge(true)
                            .force(),
                ))?;
            }
        };
    } else if analysis.0.is_normal() {
        // do a normal merge
        let head_commit = repo.reference_to_annotated_commit(&repo.head()?)?;
        normal_merge(&repo, &head_commit, &fetch_commit)?;
    } else {
        // println!("Nothing to do...");
    }
    Ok(())
}

fn run(repo_path: String) -> Result<(), git2::Error> {
    let remote_name = "origin";
    let remote_branch = "master";
    let repo = Repository::open(&repo_path)?;
    let mut remote = repo.find_remote(remote_name).or_else(|find_remote_err| {
        let remotes = repo.remotes()?;
        if remotes.len() == 1 {
            return match remotes.get(0) {
                Some(remote) => {
                    println!("using non origin remote {}", remote);
                    repo.find_remote(remote)
                },
                None => Err(find_remote_err),
            };
        } else if remotes.len() > 1 {
            println!("{}", repo_path);
            println!("multiple remotes:");
            for r in remotes.iter() {
                println!("  {:?}", r);
            }
            println!("Unable to pick between them as no \"origin\" exists.");
        } else {
            println!("{}", repo_path);
            println!("no remotes found");
        }
        Err(find_remote_err)
    })?;
    let fetch_commit = do_fetch(&repo, &[remote_branch], &mut remote, &repo_path)?;
    do_merge(&repo, &remote_branch, fetch_commit)

}

fn main() {
    let args: Vec<String> = env::args().collect();
    let repo_containers = &args[1..];
    println!("{:?}", repo_containers);

    for rc in repo_containers {
        let repo_dir = fs::read_dir(rc).unwrap();
        for repo in repo_dir {
            let repo = repo.unwrap();
            if repo.metadata().unwrap().is_dir() {
                let _ = run(repo.path().display().to_string());
            }
        }
    }
}
