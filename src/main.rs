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
use std::cmp;

// TODO Should this attempt to update submodules of repos with submodules?
// Maybe as a configurable option?
// E.g. [redox](https://gitlab.com/redox-os.org/redox-os/redox)
//
// TODO Need to add a way for authing repos.
// E.g. to resolve: Error { code: -1, klass: 34, message: "remote authentication required but no callback set" }
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
enum Outcome {
    // #TODO: Should these be consolidated? Does the user care or want to know
    // all the different failures and their reasons?
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
    NeedsResolution,
    FailedFetch,
    WIPOther // For unconsidered errors. This should eventually eliminated
}

#[derive(Debug, Clone)]
struct Upgit {
    path:   String,
    outcome: Outcome,
    report: String,
}

fn do_fetch<'a>(
    repo: &'a git2::Repository,
    refs: &[&str],
    remote: &'a mut git2::Remote,
    local_branch_name: &str,
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

    let local_branch = repo.find_branch(local_branch_name, git2::BranchType::Local)?;
    let upstream_branch = local_branch.upstream()?;
    Ok(repo.reference_to_annotated_commit(upstream_branch.get())?)
}

fn fast_forward(
    repo: &Repository,
    lb: &mut git2::Reference,
    rc: &git2::AnnotatedCommit,
    repo_path: String,
) -> Upgit {
    let mk_upgit = with_path(repo_path.clone());
    let mk_upgit_other = with_path_other(repo_path.clone());
    let remote_tree = repo.find_commit(rc.id()).unwrap().tree().unwrap();
    let local_tree = lb.peel_to_tree().unwrap();
    let mut opts = git2::DiffOptions::new();
    opts.minimal(true);
    let the_diff = repo.diff_tree_to_tree(Some(&local_tree), Some(&remote_tree), Some(&mut opts)).unwrap();
    let mut diff_report = vec![String::from("")];
    the_diff.print(
         git2::DiffFormat::NameStatus,
         |diff_delta, _option_diff_hunk, _diff_line| {
             let status = diff_delta.status();
             let old_file = diff_delta.old_file().path().unwrap_or(Path::new("/unknown")).display();
             let new_file = diff_delta.new_file().path().unwrap_or(Path::new("/unknown")).display();
             let report_str = match status {
                 git2::Delta::Unmodified => format!(""),
                 git2::Delta::Added => format!("+: {}", new_file),
                 git2::Delta::Deleted => format!("-: {}", old_file),
                 git2::Delta::Modified => format!("Δ: {}", new_file),
                 git2::Delta::Renamed => format!("→: \"{}\" -> \"{}\"", old_file, new_file),
                 git2::Delta::Copied => format!("cpy: \"{}\" -> \"{}\"", old_file, new_file),
                 git2::Delta::Ignored => format!("ign: {}", new_file),
                 git2::Delta::Untracked => format!("nochg: {}", new_file),
                 git2::Delta::Typechange => format!("chgtype: {}", new_file),
                 git2::Delta::Unreadable => format!("unreadable: {}", new_file),
                 git2::Delta::Conflicted => format!("cflct: {}", new_file),
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
        Err(err) => return mk_upgit_other(
            format!("Unable to create a reference with the same name as the given reference\n    {}", err),
        ),
        _ => {},
    };
    match repo.set_head(&name) {
        Err(err) => return mk_upgit_other(
            format!("Unable to set head:\n    {}", err),
        ),
        _ => {},
    };
    match repo.checkout_head(Some(
        git2::build::CheckoutBuilder::default()
            // force required to make the working directory actually get updated/ 
            // could add logic to handle dirty working directory states
            .force(),
    )) {
        Ok(()) => mk_upgit(Outcome::Updated, diff_report.join("\n    ")),
        Err(err) => mk_upgit(
            Outcome::NeedsResolution,
            format!("Unable to checkout head. This repo may need manual resolving. Oops.\n    {}", err),
        )
    }
}

fn normal_merge(
    repo: &Repository,
    local: &git2::AnnotatedCommit,
    remote: &git2::AnnotatedCommit,
    repo_path: String,
) -> Upgit {
    let mk_upgit = with_path(repo_path.clone());
    let mk_upgit_other = with_path_other(repo_path.clone());
    let local_tree = match repo.find_commit(local.id()).and_then(|x| { x.tree() }) {
        Ok(x) => x,
        Err(err) => return mk_upgit_other(
            format!("could not find local commit\n    {}", err),
        ),
    };
    let remote_tree = match repo.find_commit(remote.id()).and_then(|x| x.tree()) {
        Ok(x) => x,
        Err(err) => return mk_upgit_other(
            format!("could not find remote commit\n    {}", err),
        ),
    };

    // match repo.graph_descendant_of(remote.id(), local.id()) {
    //     Ok(is_ancestor) => {
    //         println!("remote is ancestor: {}", is_ancestor);
    //     },
    //     Err(err) => {
    //         println!("{:?}", err);
    //     },
    // };

    let merge_base_commit = match repo.merge_base(local.id(), remote.id()) {
        Ok(x) => x,
        Err(err) => return mk_upgit_other(
            format!("No merge base local {} and remote {}\n    {}", local.id(), remote.id(), err),
        ),
    };
    let ancestor = match repo.find_commit(merge_base_commit).and_then(|x| { x.tree() }) {
        Ok(x) => x,
        Err(err) => return mk_upgit_other(
            format!("Unable to get merge_base_commit from a found merge base commit. This should probably never happen??\n    {}", err),
        ),
    };
    let mut idx = match repo.merge_trees(&ancestor, &local_tree, &remote_tree, None) {
        Ok(x) => x,
        Err(err) => return mk_upgit_other(
            format!("Unable to merge trees\n    {}", err),
        )
    };

    if idx.has_conflicts() {
        match repo.checkout_index(Some(&mut idx), None) {
            Ok(()) => return mk_upgit(Outcome::RevertedConflict, format!("")),
            Err(err) => return mk_upgit(Outcome::UnresolvedConflict, format!("{}", err)),
        };
    };
    let oid = match idx.write_tree_to(repo) {
        Ok(x) => x,
        Err(err) => return mk_upgit_other(
            format!("Could not write merged tree to repo\n    {}", err),
        )
    };
    let result_tree = match repo.find_tree(oid) {
        Ok(x) => x,
        Err(err) => return mk_upgit_other(
            format!("Unable to find tree for the thing that was just merged. This should not happen\n    {}", err),
        ),
    };
    // now create the merge commit
    let msg = format!("Merge: {} into {}", remote.id(), local.id());
    let sig = match repo.signature() {
        Ok(x) => x,
        Err(err) => return mk_upgit_other(
            format!("Could not find signature\n    {}", err),
        )
    };
    let local_commit = match repo.find_commit(local.id()) {
        Ok(x) => x,
        Err(err) => return mk_upgit_other(
            format!("Could not find local commit\n    {}", err),
        )
    };
    let remote_commit = match repo.find_commit(remote.id()) {
        Ok(x) => x,
        Err(err) => return mk_upgit_other(
            format!("Unable to find remote commit\n    {}", err),
        )
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
        Err(err) => return mk_upgit_other(
            format!("Unable to make commit\n    {}", err),
        ),
        _ => {},
    };

    // Set working tree to match head.
    match repo.checkout_head(None) {
        Err(err) => mk_upgit_other(
            format!("Unable to checkout head\n    {}", err),
        ),
        _ => mk_upgit(Outcome::Updated, format!(""))
    }
}

fn do_merge<'a>(
    repo: &'a Repository,
    remote_branch: &str,
    fetch_commit: git2::AnnotatedCommit<'a>,
    repo_path: String
) -> Upgit {
    let mk_upgit = with_path(repo_path.clone());
    let mk_upgit_other = with_path_other(repo_path.clone());
    // 1. do a merge analysis
    let (analysis, _) = match repo.merge_analysis(&[&fetch_commit]) {
        Ok(x) => x,
        Err(err) => return mk_upgit(Outcome::FailedMergeAnalysis, format!("{:?}", err)),
    };

    // 2. Do the appropriate merge
    if analysis.is_fast_forward() {
        let refname = format!("refs/heads/{}", remote_branch);
        match repo.find_reference(&refname) {
            Ok(mut r) => return fast_forward(repo, &mut r, &fetch_commit, repo_path),
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
                    Err(err) => return mk_upgit_other(
                        format!("Unable to create reference \"{}\". Does it already exist?\n    {}", refname, err),
                    ),
                    _ => {},
                };

                match repo.set_head(&refname) {
                    Err(err) => return mk_upgit_other(format!("Unable to set head\n    {}", err)),
                    _ => {},
                };
                match repo.checkout_head(Some(
                        git2::build::CheckoutBuilder::default()
                            .allow_conflicts(true)
                            .conflict_style_merge(true)
                            .force(),
                )) {
                    Err(err) => return mk_upgit_other(format!("Unable to set head\n    {}", err)),
                    Ok(_) => return mk_upgit(Outcome::Updated, format!("")),
                };
            }
        }
    } else if analysis.is_normal() {
        // do a normal merge
        let reference = match repo.head() {
            Ok(x) => x,
            Err(err) => return mk_upgit_other(
                format!("Unable to retrieve reference pointed to by HEAD\n    {}", err),
            )
        };
        let head_commit = match repo.reference_to_annotated_commit(&reference) {
            Ok(x) => x,
            Err(err) => return mk_upgit_other(format!("unable to resolve reference\n    {}", err)),
        };
        return normal_merge(&repo, &head_commit, &fetch_commit, repo_path)
    }

    return if analysis.is_none() {
        mk_upgit_other(format!("Merge analysis is none."))
    } else if analysis.is_up_to_date() {
        mk_upgit(Outcome::UpToDate, format!(""))
    } else if analysis.is_unborn() {
        mk_upgit_other(format!("Unborn merge analysis"))
    } else {
        mk_upgit_other(format!("unknown status, this should probably not happen"))
    }
}

fn with_path(path: String) -> Box<dyn Fn(Outcome, String) -> Upgit> {
    return Box::new(move |outcome: Outcome, report: String| Upgit {
        path: path.clone(),
        outcome,
        report
    })
}

fn with_path_other(path: String) -> Box<dyn Fn(String) -> Upgit> {
    return Box::new(move |report: String| Upgit {
        path: path.clone(),
        outcome: Outcome::WIPOther,
        report
    })
}

fn with_path_no_report(path: String) -> Box<dyn Fn(Outcome) -> Upgit> {
    return Box::new(move |outcome: Outcome| Upgit {
        path: path.clone(),
        outcome,
        report: String::from(""),
    })
}

fn get_origin_remote(repo: &Repository, repo_path: String) -> Result<git2::Remote, Upgit> {
    repo.find_remote("origin").or_else(|find_remote_err| {
        let mk_upgit_no_report = with_path_no_report(repo_path.clone());
        let mk_upgit_other = with_path_other(repo_path.clone());
        let remotes = match repo.remotes() {
            Ok(r) => r,
            Err(_) => return Err(mk_upgit_no_report(Outcome::NoRemotes)),
        };

        return if remotes.len() == 1 {
            match remotes.get(0) {
                Some(remote) => {
                    match repo.find_remote(remote) {
                        Err(err) => Err(mk_upgit_other(
                            format!("Err using non origin remote {}\n    {:?}", remote, err)
                        )),
                        Ok(x) => Ok(x),
                    }
                },
                None => Err(mk_upgit_other(format!("{}", find_remote_err))),
            }
        } else if remotes.len() > 1 {
            Err(mk_upgit_no_report(Outcome::NoClearOrigin))
        } else {
            Err(mk_upgit_no_report(Outcome::NoRemotes))
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
                    dirty_things.push(format!("idx +: {}", path));
                }
                if status.is_index_modified() {
                    dirty_things.push(format!("idx Δ: {}", path));
                }
                if status.is_index_deleted() {
                    dirty_things.push(format!("idx -: {}", path));
                }
                if status.is_index_renamed() {
                    dirty_things.push(format!("idx ->: {}", path));
                }
                if status.is_index_typechange() {
                    dirty_things.push(format!("idx Δtype: {}", path));
                }

                if status.is_wt_new() {
                    dirty_things.push(format!("wt +: {}", path));
                }
                if status.is_wt_modified() {
                    dirty_things.push(format!("wt Δ: {}", path));
                }
                if status.is_wt_deleted() {
                    dirty_things.push(format!("wt -: {}", path));
                }
                if status.is_wt_renamed() {
                    dirty_things.push(format!("wt ->: {}", path));
                }
                if status.is_wt_typechange() {
                    dirty_things.push(format!("wt Δtype: {}", path));
                }

                if status.is_conflicted() {
                    dirty_things.push(format!("conflict: {}", path));
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
    let mk_upgit = with_path(repo_path.clone());

    let repo = match Repository::open(&repo_path) {
        Ok(r) => r,
        Err(_) => return mk_upgit(Outcome::NotARepo, format!("")),
    };
    let remote_branch = match repo.head() {
        Ok(the_head) => {
            match the_head.shorthand() {
                Some(x) => String::from(x),
                None => return mk_upgit(Outcome::WIPOther, format!("not able to get local head branch name")),
            }
        },
        Err(err) => return mk_upgit(Outcome::WIPOther, format!("not able to get local head branch name, {}", err)),
    };
    let mut remote = match get_origin_remote(&repo, repo_path.clone()) {
        Ok(r) => r,
        Err(upgit) => return upgit,
    };

    let dirty_status = check_repo_dirty(&repo);
    match dirty_status {
        Some(statuses) => return mk_upgit(Outcome::Dirty, statuses.join("\n    ")),
        _ => {},
    };

    let fetch_commit = match do_fetch(&repo, &[&remote_branch], &mut remote, &remote_branch) {
        Ok(x) => x,
        Err(err) => return mk_upgit(Outcome::FailedFetch, format!("{:?}", err)),
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
        println!("Not a repo ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.path);
        };
        None
    });

    groups.get(&Outcome::NoRemotes).and_then(|upgits| -> Option<()> {
        println!("No remote ({}):", upgits.len());
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

    groups.get(&Outcome::FailedFetch).and_then(|upgits| -> Option<()> {
        println!("Failed to fetch ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.path);
            println!("    {}", u.report);
        };
        None
    });

    groups.get(&Outcome::NeedsResolution).and_then(|upgits| -> Option<()> {
        println!("Needs resolution ({}):", upgits.len());
        for u in upgits {
            println!("  {}", u.path);
            println!("    {}", u.report);
        };
        None
    });

    groups.get(&Outcome::WIPOther).and_then(|upgits| -> Option<()> {
        println!("Other error outcome ({}):", upgits.len());
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
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let repo_containers = &args[1..];

    let mut counter = 0;
    for rc in repo_containers {
        // 1-8, leaving at least 2 open for other programs
        let num_threads = cmp::min(8, cmp::max(1, num_cpus::get() - 2));

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
                                    outcome: Outcome::NotARepo,
                                    report: String::from(""),
                                }
                            }
                        },
                        Err(err) => Upgit {
                            path: String::from(""),
                            outcome: Outcome::BadFsEntry,
                            report: format!("{:?}", err),
                        }
                    };
                    tx_clone.send(upgit).unwrap();
                }
            });
            children.push(child);
        }
        drop(tx);

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
