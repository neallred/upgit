use git2::Repository;
use std::str;
use std::fs;
use std::env;
use std::hash::Hash;
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::mpsc;
use std::thread;
use std::path::Path;

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
enum Outcome {
    NotARepo,
    NoRemotes,
    BadFsEntry,
    Dirty,
    RemoteHeadMismatch,
    UpToDate,
    Updated,
    NoClearOrigin,
    BareRepository,
    FailedMergeAnalysis,
    RevertedConflict,
    UnresolvedConflict,
    NeedsResolution(String),
    FailedFetch(String),
    Other(String) // For unconsidered errors.
}

#[derive(Debug, Clone)]
struct Upgit {
    outcome: Outcome,
    path:   String,
    report: String,
}

fn do_fetch<'a>(
    repo: &'a git2::Repository,
    refs: &[&str],
    remote: &'a mut git2::Remote,
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
    repo_path: String,
) -> Upgit {
    let remote_tree = repo.find_commit(rc.id()).unwrap().tree().unwrap();
    let local_tree = lb.peel_to_tree().unwrap();
    let mut opts = git2::DiffOptions::new();
    opts.minimal(true);
    let the_diff = repo.diff_tree_to_tree(Some(&local_tree), Some(&remote_tree), Some(&mut opts)).unwrap();
    // git struct Diff
    let mut diff_report = vec![String::from("")];
    the_diff.print(
         git2::DiffFormat::NameStatus,
         |diff_delta, _option_diff_hunk, _diff_line| {
             let status = diff_delta.status();
             let old_file = diff_delta.old_file().path().unwrap_or(Path::new("/unknown")).display();
             let new_file = diff_delta.new_file().path().unwrap_or(Path::new("/unknown")).display();
             let report_str = match status {
                 git2::Delta::Unmodified => format!(""),
                 git2::Delta::Added => format!("Added: {}", new_file),
                 git2::Delta::Deleted => format!("Deleted: {}", old_file),
                 git2::Delta::Modified => format!("Changed: {}", new_file),
                 git2::Delta::Renamed => format!("mv: \"{}\" -> \"{}\"", old_file, new_file),
                 git2::Delta::Copied => format!("Copied: \"{}\" -> \"{}\"", old_file, new_file),
                 git2::Delta::Ignored => format!("Ignored: {}", new_file),
                 git2::Delta::Untracked => format!("Unchanged: {}", new_file),
                 git2::Delta::Typechange => format!("Typechange: {}", new_file),
                 git2::Delta::Unreadable => format!("Unreadable: {}", new_file),
                 git2::Delta::Conflicted => format!("Conflicted: {}", new_file),
             };

             diff_report.push(report_str);

             true
         }
    ).unwrap();

    let name = match lb.name() {
        Some(s) => s.to_string(),
        None => String::from_utf8_lossy(lb.name_bytes()).to_string(),
    };
    let msg = format!("Fast-Forward: Setting {} to id: {}", name, rc.id());
    
    match lb.set_target(rc.id(), &msg) {
        Err(err) => {
            return Upgit {
                path: repo_path,
                outcome: Outcome::Other(String::from("Unable to create a reference with the same name as the given reference")),
                report: format!("{}", err),
            };
        }
        _ => {},
    };
    match repo.set_head(&name) {
        Err(err) => {
            return Upgit {
                path: repo_path,
                outcome: Outcome::Other(String::from("Unable to set head")),
                report: format!("{}", err),
            };
        },
        _ => {},
    };
    match repo.checkout_head(Some(
        git2::build::CheckoutBuilder::default()
            // force required to make the working directory actually get updated/ 
            // could add logic to handle dirty working directory states
            .force(),
    )) {
        Ok(()) => Upgit {
            path: repo_path,
            outcome: Outcome::Updated,
            report: diff_report.join("\n    "),
        },
        Err(err) => Upgit {
            path: repo_path,
            outcome: Outcome::NeedsResolution(String::from("Unable to checkout head. This repo may need manual resolving. Oops.")),
            report: format!("{}", err),
        }
    }
}

fn normal_merge(
    repo: &Repository,
    local: &git2::AnnotatedCommit,
    remote: &git2::AnnotatedCommit,
    repo_path: String,
) -> Upgit {
    let local_tree = match repo.find_commit(local.id()).and_then(|x| { x.tree() }) {
        Ok(x) => x,
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("could not find local commit")),
            report: format!("{}", err),
        },
    };
    let remote_tree = match repo.find_commit(remote.id()).and_then(|x| x.tree()) {
        Ok(x) => x,
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("could not find remote commit")),
            report: format!("{}", err),
        },
    };
    let merge_base_commit = match repo.merge_base(local.id(), remote.id()) {
        Ok(x) => x,
        Err(err) => {
            println!("\n{}: local.id() = {}, remote.id() = {}", repo_path, local.id(), remote.id());
            return Upgit {
                path: repo_path,
                outcome: Outcome::Other(String::from("Unable to find a merge base between two commits")),
                report: format!("{}", err),
            }
        }
    };
    let ancestor = match repo.find_commit(merge_base_commit).and_then(|x| { x.tree() }) {
        Ok(x) => x,
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("Unable to get merge_base_commit from a found merge base commit. This should probably never happen??")),
            report: format!("{}", err),
        },
    };
    let mut idx = match repo.merge_trees(&ancestor, &local_tree, &remote_tree, None) {
        Ok(x) => x,
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("Unalbe to merge trees")),
            report: format!("{}", err),
        }
    };

    if idx.has_conflicts() {
        match repo.checkout_index(Some(&mut idx), None) {
            Ok(()) => return Upgit {
                path: repo_path,
                outcome: Outcome::RevertedConflict,
                report: String::from(""),
            },
            Err(err) => return Upgit {
                path: repo_path,
                outcome: Outcome::UnresolvedConflict,
                report: format!("{}", err),
            },
        };
    };
    let oid = match idx.write_tree_to(repo) {
        Ok(x) => x,
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("could not write merged tree to repo")),
            report: format!("{}", err),
        }
    };
    let result_tree = match repo.find_tree(oid) {
        Ok(x) => x,
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("Unable to find tree for the thing that was just merged. This should not happen")),
            report: format!("{}", err),
        },
    };
    // now create the merge commit
    let msg = format!("Merge: {} into {}", remote.id(), local.id());
    let sig = match repo.signature() {
        Ok(x) => x,
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("could not find signature")),
            report: format!("{}", err),
        }
    };
    let local_commit = match repo.find_commit(local.id()) {
        Ok(x) => x,
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("Could not find local commit")),
            report: format!("{}", err),
        }
    };
    let remote_commit = match repo.find_commit(remote.id()) {
        Ok(x) => x,
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("Unable to find remote commit")),
            report: format!("{}", err),
        }
    };
    // do our merge commit and set current branch head to that commit.
    let _merge_commit = match repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &msg,
        &result_tree,
        &[&local_commit, &remote_commit],
    ) {
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("Unable to make commit")),
            report: format!("{}", err),
        },
        _ => {},
    };

    // Set working tree to match head.
    match repo.checkout_head(None) {
        Err(err) => Upgit {
            path: repo_path,
            outcome: Outcome::Other(String::from("Unable to checkout head")),
            report: format!("{}", err),
        },
        _ => Upgit {
            path: repo_path,
            outcome: Outcome::Updated,
            report: String::from(""),
        }
    }
}

fn do_merge<'a>(
    repo: &'a Repository,
    remote_branch: &str,
    fetch_commit: git2::AnnotatedCommit<'a>,
    repo_path: String
) -> Upgit {
    // 1. do a merge analysis
    let (analysis, _) = match repo.merge_analysis(&[&fetch_commit]) {
        Ok(x) => x,
        Err(err) => return Upgit {
            path: repo_path,
            outcome: Outcome::FailedMergeAnalysis,
            report: format!("{:?}", err),
        },
    };

    // 2. Do the appropriate merge
    if analysis.is_fast_forward() {
        let refname = format!("refs/heads/{}", remote_branch);
        match repo.find_reference(&refname) {
            Ok(mut r) => fast_forward(repo, &mut r, &fetch_commit, repo_path),
            Err(_) => {
                // The branch doesn't exist so just set the reference to the
                // commit directly. Usually this is because you are
                // pulling into an empty repository.
                match repo.reference(
                    &refname,
                    fetch_commit.id(),
                    true,
                    &format!("Setting {} to {}", remote_branch, fetch_commit.id()),
                ) {
                    Err(err) => {
                        return Upgit {
                            path: repo_path,
                            outcome: Outcome::Other(format!("Unable to create reference \"{}\". Does it already exist?", refname)),
                            report: format!("{}", err),
                        };
                    },
                    _ => {},
                };

                match repo.set_head(&refname) {
                    Err(err) => {
                        return Upgit {
                            path: repo_path,
                            outcome: Outcome::Other(String::from("Unable to set head")),
                            report: format!("{}", err),
                        };
                    },
                    _ => {},
                };
                match repo.checkout_head(Some(
                        git2::build::CheckoutBuilder::default()
                            .allow_conflicts(true)
                            .conflict_style_merge(true)
                            .force(),
                )) {
                    Err(err) => return Upgit {
                        path: repo_path,
                        outcome: Outcome::Other(String::from("Unable to set head")),
                        report: format!("{}", err),
                    },
                    Ok(()) => return Upgit {
                        path: repo_path,
                        outcome: Outcome::Updated,
                        report: String::from(""),
                    },
                }
            }
        }
    } else if analysis.is_normal() {
        // do a normal merge
        let reference = match repo.head() {
            Ok(x) => x,
            Err(err) => return Upgit {
                path: repo_path,
                outcome: Outcome::Other(String::from("Unable to retrieve reference pointed to by HEAD")),
                report: format!("{}", err),
            }
        };
        let head_commit = match repo.reference_to_annotated_commit(&reference) {
            Ok(x) => x,
            Err(err) => return Upgit {
                path: repo_path,
                outcome: Outcome::Other(String::from("unable to resolve reference")),
                report: format!("{}", err),
            },
        };
        normal_merge(&repo, &head_commit, &fetch_commit, repo_path)
    } else if analysis.is_none() {
       return Upgit{
            path: repo_path,
            outcome: Outcome::Other(String::from("Merge analysis is none...")),
            report: String::from(""),
        };
    } else if analysis.is_up_to_date() {
       return Upgit{
            path: repo_path,
            outcome: Outcome::UpToDate,
            report: String::from(""),
        };
    } else if analysis.is_unborn() {
       return Upgit{
            path: repo_path,
            outcome: Outcome::Other(String::from("Unborn merge analysis")),
            report: String::from(""),
        };
    } else {
       return Upgit{
            path: repo_path,
            outcome: Outcome::Other(String::from("unknown status, this should probably not happen")),
            report: String::from(""),
        };
    }
}

fn get_origin_remote(repo: &Repository) -> Result<git2::Remote, Outcome> {
    repo.find_remote("origin").or_else(|find_remote_err| {
        let remotes = match repo.remotes() {
            Ok(r) => r,
            Err(_) => {
                return Err(Outcome::NoRemotes)
            },
        };

        return if remotes.len() == 1 {
            match remotes.get(0) {
                Some(remote) => {
                    println!("using non origin remote {}", remote);
                    match repo.find_remote(remote) {
                        Ok(remote) => Ok(remote),
                        Err(err) => Err(Outcome::Other(format!("Why u no remote? {:?}", err)))
                    }
                },
                None => Err(Outcome::Other(format!("{}", find_remote_err))),
            }
        } else if remotes.len() > 1 {
            Err(Outcome::NoClearOrigin)
        } else {
            Err(Outcome::NoRemotes)
        }
    })
}

fn check_repo_dirty(repo: &Repository) -> Option<Vec<String>> {
    let mut dirty_things = vec![];
    let result_statuses = repo.statuses(None);
    match result_statuses {
        Ok(statuses) => {
            for status_entry in statuses.iter() {
                let status = status_entry.status();
                let path = status_entry.path().unwrap_or("");
                if status.is_index_new() {
                    dirty_things.push(format!("index new: {}", path));
                }
                if status.is_index_modified() {
                    dirty_things.push(format!("index modified: {}", path));
                }
                if status.is_index_deleted() {
                    dirty_things.push(format!("index deleted: {}", path));
                }
                if status.is_index_renamed() {
                    dirty_things.push(format!("index renamed: {}", path));
                }
                if status.is_index_typechange() {
                    dirty_things.push(format!("index typechange: {}", path));
                }

                if status.is_wt_new() {
                    dirty_things.push(format!("wt new: {}", path));
                }
                if status.is_wt_modified() {
                    dirty_things.push(format!("wt modified: {}", path));
                }
                if status.is_wt_deleted() {
                    dirty_things.push(format!("wt deleted: {}", path));
                }
                if status.is_wt_renamed() {
                    dirty_things.push(format!("wt renamed: {}", path));
                }
                if status.is_wt_typechange() {
                    dirty_things.push(format!("wt typechange: {}", path));
                }

                if status.is_conflicted() {
                    dirty_things.push(format!("conflicted: {}", path));
                }
            }
        },
        Err(err) => dirty_things.push(format!("statuses err: {}", err)),
    };

    if dirty_things.len() > 0 {
        Some(dirty_things)
    } else {
        None
    }
}

fn run(repo_path: String) -> Upgit {
    let remote_branch = "master";
    let repo = match Repository::open(&repo_path) {
        Ok(r) => r,
        Err(_) => {
            return Upgit {
                outcome: Outcome::NotARepo,
                path:   format!("{}", repo_path),
                report: String::from(""),
            }
        },
    };
    let mut remote = match get_origin_remote(&repo) {
        Ok(r) => r,
        Err(outcome) => {
            return Upgit {
                path: repo_path,
                outcome,
                report: String::from(""),
            };
        },
    };

    let dirty_status = check_repo_dirty(&repo);
    match dirty_status {
        Some(statuses) => {
            return Upgit {
                path: repo_path,
                outcome: Outcome::Dirty,
                report: statuses.join("\n    "),
            }
        },
        _ => {},
    };

    let fetch_commit = match do_fetch(&repo, &[remote_branch], &mut remote) {
        Ok(x) => x,
        Err(err) => {
            return Upgit {
                outcome: Outcome::FailedFetch(format!("{:?}", err)),
                path:   format!("{}", repo_path),
                report: String::from(""),
            }
        },
    };
    return do_merge(&repo, &remote_branch, fetch_commit, repo_path)
}

fn group_upgits(upgits: Vec<Upgit>) -> HashMap<Outcome, Vec<Upgit>> {
    let mut grouped: HashMap<Outcome, Vec<Upgit>> = HashMap::new();
    upgits.into_iter().fold(&mut grouped, move |grouped, u| {
        match grouped.get_mut(&u.outcome) {
            Some(mutable_group) => {
                mutable_group.push(u);
            },
            None => {
                let _ = grouped.insert(u.outcome.clone(), vec![u]);
            },
        };
        grouped
    });
    grouped
}

fn print_results(upgits: &Vec<Upgit>) {
    let groups = group_upgits(upgits.clone());
    println!("processed {} repos", upgits.len());
    groups.get(&Outcome::NotARepo).and_then(|upgits| -> Option<()> {
        println!("not a repo ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.path);
        };
        None
    });

    groups.get(&Outcome::NoRemotes).and_then(|upgits| -> Option<()> {
        println!("no remote ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.path);
        };
        None
    });

    groups.get(&Outcome::NoClearOrigin).and_then(|upgits| -> Option<()> {
        println!("No clear remote origin ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.path);
            println!("{}", u.report);
        };
        None
    });

    groups.get(&Outcome::BareRepository).and_then(|upgits| -> Option<()> {
        println!("Bare repository, not updating ({}):", upgits.len());
        None
    });

    groups.get(&Outcome::BadFsEntry).and_then(|upgits| -> Option<()> {
        println!("Bad filesystem ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.report);
        };
        None
    });

    groups.get(&Outcome::RemoteHeadMismatch).and_then(|upgits| -> Option<()> {
        println!("Remote head mismatch ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.path);
        };
        None
    });

    groups.get(&Outcome::UpToDate).and_then(|upgits| -> Option<()> {
        println!("Up to date ({}):", upgits.len());
        None
    });

    groups.get(&Outcome::FailedMergeAnalysis).and_then(|upgits| -> Option<()> {
        println!("Failed merge analysis ({}):", upgits.len());
        None
    });

    groups.get(&Outcome::RevertedConflict).and_then(|upgits| -> Option<()> {
        println!("Reverted conflict ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.path);
        };
        None
    });

    groups.get(&Outcome::UnresolvedConflict).and_then(|upgits| -> Option<()> {
        println!("Unresolved conflict ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.path);
        };
        None
    });

    groups.get(&Outcome::Dirty).and_then(|upgits| -> Option<()> {
        println!("Dirty, unable to update ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.path);
            println!("    {}", u.report);
        };
        None
    });

    groups.get(&Outcome::Updated).and_then(|upgits| -> Option<()> {
        println!("Updated ({}):", upgits.len());
        for u in upgits {
            println!("");
            println!("{}:", u.path);
            println!("----------------------");
            println!("{}", u.report);
            println!("");
        };
        None
    });

    for (g, upgits) in groups {
        match g {
            Outcome::FailedFetch(_) => {
                println!("failed fetch ({}):", upgits.len());
                for u in upgits {
                    println!("{}", u.path);
                    println!("{:?}", u.outcome);
                    println!("{}", u.report);
                }
            },
            Outcome::Other(_) => {
                println!("other error ({})", upgits.len());
                for u in upgits {
                    println!("{}", u.path);
                    println!("{:?}", u.outcome);
                    println!("{}", u.report);
                }
            },
            Outcome::NeedsResolution(_) => {
                println!("needs resolution ({}):", upgits.len());
                for u in upgits {
                    println!("{}", u.path);
                    println!("{:?}", u.outcome);
                    println!("{}", u.report);
                }
            },
            _ => {}
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let repo_containers = &args[1..];

    let mut counter = 0;
    for rc in repo_containers {
        let num_threads = 5;

        print!("Upgitting {}:", rc);
        io::stdout().flush().unwrap();
        let (tx, rx) = mpsc::channel();

        let upgits_vec: Vec<_> = fs::read_dir(rc).unwrap().collect(); 
        let num_repos = upgits_vec.len();
        let workload_size = num_repos / num_threads;
        let mut children = Vec::new();
        
        for thread_i in 1..(num_threads+1) {
            let rc_clone = rc.clone();
            let tx_clone = mpsc::Sender::clone(&tx);
            let child = thread::spawn(move || {
                let begin = (thread_i - 1) * workload_size;
                let take = if thread_i == num_threads {
                    num_repos 
                } else {
                    workload_size
                };
                let repos: Vec<_> = fs::read_dir(rc_clone).unwrap().skip(begin).take(take).collect(); 
                for repo in repos {
                    let upgit = match repo {
                        Ok(repo) => {
                            let repo_path = repo.path().display().to_string();
                            if repo.metadata().unwrap().is_dir() {
                                run(repo_path)
                            } else {
                                Upgit {
                                    path: repo_path,
                                    report: String::from(""),
                                    outcome: Outcome::NotARepo,
                                }
                            }
                        },
                        Err(err) => Upgit {
                            path: String::from(""),
                            report: format!("{:?}", err),
                            outcome: Outcome::BadFsEntry,
                        }
                    };
                    // println!("{}", thread_i);
                    tx_clone.send(upgit).unwrap();
                }
            });
            children.push(child);
        }
        drop(tx);
        // for child in children {
        //     child.join().expect("oops! the child thread panicked");
        // }

        let mut upgits = vec![];
        for upgit in rx {
            counter += 1;
            print!("\rUpgitting {}: {} of {}", rc, counter, num_repos);
            io::stdout().flush().unwrap();
            upgits.push(upgit); 
        };
        print_results(&upgits);
    }
}
