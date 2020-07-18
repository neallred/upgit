use std::hash::Hash;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct End {
    path:   String,
    status: Status,
    report: String,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub enum Status {
    // #TODO: Should these be consolidated? Does the user care or want to know
    // all the different failures and their reasons?
    NonRepo,
    NoRemotes,
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

pub fn non_repo(path: String, report: String) -> End {
    End {
        status: Status::NonRepo,
        path,
        report,
    }
}

pub fn with_path(path: String) -> Box<dyn Fn(Status, String) -> End> {
    return Box::new(move |status, report| End {
        path: path.clone(),
        status,
        report
    })
}

pub fn other(path: String) -> Box<dyn Fn(String) -> End> {
    return Box::new(move |report| End {
        path: path.clone(),
        status: Status::WIPOther,
        report
    })
}

pub fn sans_report(path: String) -> Box<dyn Fn(Status) -> End> {
    return Box::new(move |status| End {
        path: path.clone(),
        status,
        report: String::from(""),
    })
}

fn group(ends: Vec<End>) -> HashMap<Status, Vec<End>> {
    let mut grouped: HashMap<Status, Vec<End>> = HashMap::new();
    ends.into_iter().fold(&mut grouped, move |acc, u| {
        match acc.get_mut(&u.status) {
            Some(mutable_group) => {
                mutable_group.push(u);
            },
            None => {
                let _ = acc.insert(u.status.clone(), vec![u]);
            },
        };
        acc
    });
    grouped
}

fn print_all(ends: &Vec<End>, label: &str) -> Option<()> {
        println!("{} ({}):", label, ends.len());
        for x in ends {
            println!("  {}\n    {}", x.path, x.report);
        };
        None
}

fn print_path(ends: &Vec<End>, label: &str) -> Option<()> {
        println!("{} ({}):", label, ends.len());
        for x in ends {
            println!("  {}", x.path);
        };
        None
}

fn print_count(ends: &Vec<End>, label: &str) -> Option<()> {
        println!("{} ({})", label, ends.len());
        None
}

pub fn print(ends: &Vec<End>) {
    println!("");
    let groups = group(ends.clone());
    groups.get(&Status::NonRepo).and_then(|x| print_path(x, "Not a repo"));
    groups.get(&Status::NoRemotes).and_then(|x| print_path(x, "No remote"));
    groups.get(&Status::NoClearOrigin).and_then(|x| print_all(x, "No clear remote origin"));
    groups.get(&Status::BareRepository).and_then(|x| print_count(x, "Bare repo, skipped"));
    groups.get(&Status::RemoteHeadMismatch).and_then(|x| print_path(x, "Remote head mismatch"));
    groups.get(&Status::UpToDate).and_then(|x| print_count(x, "Up to date"));
    groups.get(&Status::FailedMergeAnalysis).and_then(|x| print_count(x, "Failed merge analysis"));
    groups.get(&Status::RevertedConflict).and_then(|x| print_all(x, "Reverted conflict"));
    groups.get(&Status::UnresolvedConflict).and_then(|x| print_all(x, "Unresolved conflict"));
    groups.get(&Status::Dirty).and_then(|x| print_all(x, "Dirty, skipped"));
    groups.get(&Status::FailedFetch).and_then(|x| print_all(x, "Couldn't fetch"));
    groups.get(&Status::NeedsResolution).and_then(|x| print_all(x, "Needs resolution"));
    groups.get(&Status::WIPOther).and_then(|x| print_all(x, "Other error"));
    groups.get(&Status::Updated).and_then(|ends| -> Option<()> {
        println!("Updated ({}):", ends.len());
        for x in ends {
            println!("{}:{}\n", x.path, x.report);
        };
        None
    });
}
