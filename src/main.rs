use git2::{Repository};
use std::fs;
use std::io;
use std::io::prelude::*;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::path::Path;
use tokio;
mod creds;
mod end;
mod config;

// TODO Should this attempt to update submodules of repos with submodules?
// Maybe as a configurable option?
// E.g. [redox](https://gitlab.com/redox-os.org/redox-os/redox)

type SharedData = Arc<Mutex<creds::Storage>>;

fn do_fetch<'a>(
    repo: &'a git2::Repository,
    refs: &[&str],
    remote: &'a mut git2::Remote,
    local_branch_name: &str,
    shared_data: SharedData,
) -> Result<git2::AnnotatedCommit<'a>, git2::Error> {
    let mut cb = git2::RemoteCallbacks::new();

    let mut fo = git2::FetchOptions::new();
    cb.credentials(|url, username, allowed_types| creds::callback(url, username, allowed_types, Arc::clone(&shared_data)) );

    fo.remote_callbacks(cb);
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
) -> end::End {
    let mk_end = end::with_path(repo_path.clone());
    let mk_other_end = end::other(repo_path.clone());
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

    if let Err(err) = lb.set_target(rc.id(), &msg) {
        return mk_other_end(
            format!("Unable to create a reference with the same name as the given reference\n    {}", err),
        )
    };
    if let Err(err) = repo.set_head(&name) {
         return mk_other_end(format!("Unable to set head:\n    {}", err))
    }
    match repo.checkout_head(Some(
        git2::build::CheckoutBuilder::default()
            // force required to make the working directory actually get updated/
            // could add logic to handle dirty working directory states
            .force(),
    )) {
        Ok(()) => mk_end(end::Status::Updated, diff_report.join("\n    ")),
        Err(err) => mk_end(
            end::Status::NeedsResolution,
            format!("Unable to checkout head. This repo may need manual resolving. Oops.\n    {}", err),
        )
    }
}

fn normal_merge(
    repo: &Repository,
    local: &git2::AnnotatedCommit,
    remote: &git2::AnnotatedCommit,
    repo_path: String,
) -> end::End {
    let mk_end = end::with_path(repo_path.clone());
    let mk_other_end = end::other(repo_path.clone());
    let local_id = local.id();
    let remote_id = remote.id();
    let local_tree = match repo.find_commit(local_id).and_then(|x| { x.tree() }) {
        Ok(x) => x,
        Err(err) => return mk_other_end(
            format!("could not find local commit\n    {}", err),
        ),
    };
    let remote_tree = match repo.find_commit(remote_id).and_then(|x| x.tree()) {
        Ok(x) => x,
        Err(err) => return mk_other_end(
            format!("could not find remote commit\n    {}", err),
        ),
    };

    let merge_base_commit = match repo.merge_base(local_id, remote_id) {
        Ok(x) => x,
        Err(err) => return mk_other_end(
            format!("No merge base local {} and remote {}\n    {}", local_id, remote_id, err),
        ),
    };
    let ancestor = match repo.find_commit(merge_base_commit).and_then(|x| { x.tree() }) {
        Ok(x) => x,
        Err(err) => return mk_other_end(
            format!("Unable to get merge_base_commit from a found merge base commit. This should probably never happen??\n    {}", err),
        ),
    };
    let mut idx = match repo.merge_trees(&ancestor, &local_tree, &remote_tree, None) {
        Ok(x) => x,
        Err(err) => return mk_other_end(
            format!("Unable to merge trees\n    {}", err),
        )
    };

    if idx.has_conflicts() {
        match repo.checkout_index(Some(&mut idx), None) {
            Ok(()) => return mk_end(end::Status::RevertedConflict, format!("")),
            Err(err) => return mk_end(end::Status::UnresolvedConflict, format!("{}", err)),
        };
    };
    let oid = match idx.write_tree_to(repo) {
        Ok(x) => x,
        Err(err) => return mk_other_end(
            format!("Could not write merged tree to repo\n    {}", err),
        )
    };
    let result_tree = match repo.find_tree(oid) {
        Ok(x) => x,
        Err(err) => return mk_other_end(
            format!("Unable to find tree for the thing that was just merged. This should not happen\n    {}", err),
        ),
    };
    // now create the merge commit
    let msg = format!("Merge: {} into {}", remote.id(), local.id());
    let sig = match repo.signature() {
        Ok(x) => x,
        Err(err) => return mk_other_end(
            format!("Could not find signature\n    {}", err),
        )
    };
    let local_commit = match repo.find_commit(local.id()) {
        Ok(x) => x,
        Err(err) => return mk_other_end(
            format!("Could not find local commit\n    {}", err),
        )
    };
    let remote_commit = match repo.find_commit(remote.id()) {
        Ok(x) => x,
        Err(err) => return mk_other_end(
            format!("Unable to find remote commit\n    {}", err),
        )
    };
    // do our merge commit and set current branch head to that commit.
    if let Err(err) = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &msg,
        &result_tree,
        &[&local_commit, &remote_commit],
    ) {
        return mk_other_end(format!("Unable to make commit\n    {}", err))
    };

    // Set working tree to match head.
    match repo.checkout_head(None) {
        Err(err) => mk_other_end(
            format!("Unable to checkout head\n    {}", err),
        ),
        _ => mk_end(end::Status::Updated, format!(""))
    }
}

fn do_merge<'a>(
    repo: &'a Repository,
    remote_branch: &str,
    fetch_commit: git2::AnnotatedCommit<'a>,
    repo_path: String
) -> end::End {
    let mk_end = end::with_path(repo_path.clone());
    let mk_other_end = end::other(repo_path.clone());
    // 1. do a merge analysis
    let (analysis, _) = match repo.merge_analysis(&[&fetch_commit]) {
        Ok(x) => x,
        Err(err) => return mk_end(end::Status::FailedMergeAnalysis, format!("{:?}", err)),
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
                let set_ref_msg = format!("Setting {} to {}", remote_branch, fetch_commit.id());
                if let Err(err) = repo.reference(&refname, fetch_commit.id(), true, &set_ref_msg) {
                    return mk_other_end(
                        format!("Can't create ref \"{}\". Does it already exist?\n    {}", refname, err)
                    )
                };

                if let Err(err) = repo.set_head(&refname) {
                    return mk_other_end(format!("Unable to set head\n    {}", err))
                };
                return match repo.checkout_head(Some(
                        git2::build::CheckoutBuilder::default()
                            .allow_conflicts(true)
                            .conflict_style_merge(true)
                            .force(),
                )) {
                    Err(err) => mk_other_end(format!("Unable to set head\n    {}", err)),
                    Ok(_) => mk_end(end::Status::Updated, format!("")),
                };
            }
        }
    } else if analysis.is_normal() {
        // do a normal merge
        let reference = match repo.head() {
            Ok(x) => x,
            Err(err) => return mk_other_end(
                format!("Unable to retrieve reference pointed to by HEAD\n    {}", err),
            )
        };
        let head_commit = match repo.reference_to_annotated_commit(&reference) {
            Ok(x) => x,
            Err(err) => return mk_other_end(format!("unable to resolve reference\n    {}", err)),
        };
        return normal_merge(&repo, &head_commit, &fetch_commit, repo_path)
    }

    return if analysis.is_none() {
        mk_other_end(format!("Merge analysis is none."))
    } else if analysis.is_up_to_date() {
        mk_end(end::Status::UpToDate, format!(""))
    } else if analysis.is_unborn() {
        mk_other_end(format!("Unborn merge analysis"))
    } else {
        mk_other_end(format!("unknown status, this should probably not happen"))
    }
}

fn get_origin_remote(repo: &Repository, repo_path: String) -> Result<git2::Remote, end::End> {
    repo.find_remote("origin").or_else(|find_remote_err| {
        let mk_end_no_report = end::sans_report(repo_path.clone());
        let mk_other_end = end::other(repo_path.clone());
        let remotes = match repo.remotes() {
            Ok(r) => r,
            Err(_) => return Err(mk_end_no_report(end::Status::NoRemotes)),
        };

        return if remotes.len() == 1 {
            match remotes.get(0) {
                Some(remote) => {
                    match repo.find_remote(remote) {
                        Err(err) => Err(mk_other_end(
                            format!("Err using non origin remote {}\n    {:?}", remote, err)
                        )),
                        Ok(x) => Ok(x),
                    }
                },
                None => Err(mk_other_end(format!("{}", find_remote_err))),
            }
        } else if remotes.len() > 1 {
            Err(mk_end_no_report(end::Status::NoClearOrigin))
        } else {
            Err(mk_end_no_report(end::Status::NoRemotes))
        }
    })
}

fn check_repo_dirty(repo: &Repository) -> Option<Vec<String>> {
    let mut dirty_things = vec![];
    let result_statuses = repo.statuses(None);
    match result_statuses {
        Ok(statuses) => for status_entry in statuses.iter() {
            let status = status_entry.status();
            let path = status_entry.path().unwrap_or("");
            if status.is_index_new() {
                dirty_things.push(format!("idx +: {}", path));
            } else if status.is_index_modified() {
                dirty_things.push(format!("idx Δ: {}", path));
            } else if status.is_index_deleted() {
                dirty_things.push(format!("idx -: {}", path));
            } else if status.is_index_renamed() {
                dirty_things.push(format!("idx ->: {}", path));
            } else if status.is_index_typechange() {
                dirty_things.push(format!("idx Δtype: {}", path));
            }

            if status.is_wt_new() {
                dirty_things.push(format!("wt +: {}", path));
            } else if status.is_wt_modified() {
                dirty_things.push(format!("wt Δ: {}", path));
            } else if status.is_wt_deleted() {
                dirty_things.push(format!("wt -: {}", path));
            } else if status.is_wt_renamed() {
                dirty_things.push(format!("wt ->: {}", path));
            } else if status.is_wt_typechange() {
                dirty_things.push(format!("wt Δtype: {}", path));
            }

            if status.is_conflicted() {
                dirty_things.push(format!("conflict: {}", path));
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

fn run(repo_path: String, shared_data: SharedData) -> end::End {
    let mk_end = end::with_path(repo_path.clone());
    let mk_other_end = end::other(repo_path.clone());

    let repo = match Repository::open(&repo_path) {
        Ok(r) => r,
        Err(_) => return end::non_repo(repo_path, format!("")),
    };
    let remote_branch = match repo.head() {
        Ok(the_head) => {
            match the_head.shorthand() {
                Some(x) => String::from(x),
                None => return mk_other_end(format!("Can't get local head name")),
            }
        },
        Err(err) => return mk_other_end(format!("Can't get local head name, {}", err)),
    };
    let mut remote = match get_origin_remote(&repo, repo_path.clone()) {
        Ok(r) => r,
        Err(end) => return end,
    };

    if let Some(statuses) = check_repo_dirty(&repo) {
         return mk_end(end::Status::Dirty, statuses.join("\n    "))
    };

    // Up to here, no network calls are made
    let fetch_commit = match do_fetch(&repo, &[&remote_branch], &mut remote, &remote_branch, shared_data) {
        Ok(x) => x,
        Err(err) => return mk_end(end::Status::FailedFetch, format!("{:?}", err)),
    };
    return do_merge(&repo, &remote_branch, fetch_commit, repo_path)
}

#[tokio::main]
async fn main() {
    let config = config::new();
    println!("config: {:?}", config);

    let shared_data: SharedData = Arc::new(Mutex::new(creds::Storage::from_config(&config)));

    for gd in config.git_dirs {

        print!("Upgitting {}:", gd);
        io::stdout().flush().unwrap();
        let (tx, rx) = mpsc::channel();

        let (mut non_repo_ends, repos) = fs::read_dir(&gd).unwrap().fold((Vec::new(), Vec::new()), |(mut ends, mut repos), fs_entry| {
            match fs_entry {
                Ok(repo) => {
                    let repo_path = repo.path().display().to_string();
                    if repo.metadata().unwrap().is_dir() {
                        repos.push(repo_path);
                    } else {
                        ends.push(end::non_repo(repo_path, format!("")));
                    }
                },
                Err(err) => {
                    ends.push(end::non_repo(
                            String::from("Unknown fs entity"),
                            format!("{:?}", err)
                    ));
                }
            };
            (ends, repos)
        });
        let mut counter = non_repo_ends.len();

        let num_repos = counter + repos.len();

        for r in repos {
            let tx_clone = mpsc::Sender::clone(&tx);
            let shared_data_clone = Arc::clone(&shared_data);

            tokio::spawn(async move {
                let end = run(r, Arc::clone(&shared_data_clone));
                tx_clone.send(end).unwrap();
            });
        }
        drop(tx);

        let mut ends = vec![];
        for end in rx {
            counter += 1;
            // Do not output here if arc structure is being interacted with,
            // as it might mean user is being promted for input.
            if let Ok(_) = shared_data.try_lock() {
                print!("\rUpgitting {}: {} of {}", gd, counter, num_repos);
                io::stdout().flush().unwrap();
            }
            ends.push(end);
        };
        ends.append(&mut non_repo_ends);
        end::print(&ends);
    }
}
