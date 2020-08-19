use git2::{Cred, CredentialType};
use std::path::Path;
use rpassword;
use std::collections::{HashMap, HashSet};
use crate::config;
use std::env;
use text_io::read;
use url;
use std::fmt;
use crate::string_ops;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum GitCred {
    Plain(String), // PW for that user/url combo
    Ssh(String, Option<String>), // path to ssh key, optional passphrase
}

type Seen = HashSet<GitCred>;

#[derive(Debug, Clone)]
struct RepoCred {
    active: GitCred,
    seen: Seen,
}

type RepoGraph = HashMap<String, Domain>; // domain -> Domain
type Domain = HashMap<String, Org>; // org -> Org
type Org = HashMap<String, Repo>; // repo -> Repo
type Repo = HashMap<String, RepoCred>; // repo_path -> RepoCred

#[derive(Debug, Clone)]
pub struct Storage {
    default_ssh: Option<GitCred>, // code ensures its an ssh
    default_plain: Option<GitCred>, // code ensures its a plaintext
    repo_graph: RepoGraph,
    keys: HashSet<GitCred>, // code ensures it is only ssh keys
    share: config::Share,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitUrl {
    domain: String,
    org: String,
    repo: String,
    scheme: String,
    username: String,
}

#[derive(Debug)]
enum UpgitErr {
    UrlParse(String)
}

impl fmt::Display for UpgitErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &*self {
            UpgitErr::UrlParse(x) => write!(f, "{}", x.clone())
        }
    }
}

fn mk_err(x: &str) -> Box<UpgitErr> {
    Box::new(UpgitErr::UrlParse(String::from(x)))
}


impl std::error::Error for UpgitErr {
    fn description(&self) -> &str {
        match &*self {
            UpgitErr::UrlParse(x) => x.as_str()
        }
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        None
    }
}

fn parse_git_url(input_url: String) -> Result<GitUrl, Box<dyn std::error::Error>> {
    // e.g. ssh.
    let parts: Vec<_> = input_url.split('@').collect();
    if parts.len() < 1 {
        return Err(mk_err("missing user"));
    }
    if parts.len() < 2 {
        return Err(mk_err("did not have other info"));
    }
    let domain_parts: Vec<_> = parts[1].split(':').collect();
    if domain_parts.len() < 1 {
        return Err(mk_err("did not have domain"));
    }
    if domain_parts.len() < 2 {
        return Err(mk_err("did not have other info"));
    }
    let org_domain_parts: Vec<_> = domain_parts[1].split('/').collect();
    let (org, repo) = match org_domain_parts.len() {
        0 => (String::from(""), String::from("")),
        1 => (String::from(""), org_domain_parts[0].to_string()),
        2 => (org_domain_parts[0].to_string(), org_domain_parts[1].to_string()),
        x => (org_domain_parts[0..(x - 2)].to_vec().join("/"), org_domain_parts[x - 1].to_string()),
    };

    return Ok(GitUrl{
        domain: domain_parts[0].to_string(),
        org,
        repo,
        scheme: String::from("git"),
        username: parts[0].to_string(),
    });
}

fn parse_url(input_url: String) -> Result<GitUrl, Box<dyn std::error::Error>> {
    let mut with_protocol = input_url.clone();
    if with_protocol.starts_with(&String::from("git@")) {
        return parse_git_url(input_url);
    }

    if !with_protocol.starts_with(&String::from("https://")) &&
        !with_protocol.starts_with(&String::from("http://"))
    {
        with_protocol = String::from("unknown://") + &with_protocol;
    }
    let url = url::Url::parse(&with_protocol)?;
    let segments = url.path_segments().map(|c| c.collect::<Vec<_>>()).unwrap_or(vec![]);
    let mut org = String::from("");
    let mut repo = String::from("");
    let num_segments = segments.len();
    if num_segments == 1 {
        repo = segments[0].to_string();
    } else if num_segments > 1 {
        org = segments[0..(num_segments - 1)].to_vec().join("/");
        repo = segments[num_segments - 1].to_string();
    }
    Ok(GitUrl{
        domain: url.host_str().unwrap_or(&String::from("")).to_string(),
        org,
        repo,
        scheme: url.scheme().to_string(),
        username: url.username().to_string(),
    })
}

fn get_shared_pwd_repo(repo: &Repo, repo_path: &String, seen: &Seen) -> Option<GitCred> {
    repo.iter()
        .find(|(k, v)| *k != repo_path && !seen.contains(&v.active))
        .map(|(_, v)| v.active.clone())
}

fn get_shared_pwd_org(org: &Org, repo_path: &String, seen: &Seen) -> Option<GitCred> {
    org.iter().find_map(|(_, v)| get_shared_pwd_repo(v, &repo_path, seen))
}

fn get_shared_pwd_domain(domain: &Domain, repo_path: &String, seen: &Seen) -> Option<GitCred> {
    domain.iter().find_map(|(_, v)| get_shared_pwd_org(v, &repo_path, seen))
}

// Looks within the tree for an existing, that isn't self
fn get_shared_cred(rg: &RepoGraph, share: &config::Share, git_url: GitUrl, repo_path: &String, seen: &Seen) -> Option<GitCred> {
    // Need to pass in seen because of how updating seen works currently.
    if share == &config::Share::Never || share == &config::Share::Defaults {
        return None
    };

    let domain_key = url_to_domain(&git_url);
    if let Some(domain) = rg.get(&domain_key) {
        if let Some(org) = domain.get(&git_url.org) {
            if let Some(repo) = org.get(&git_url.repo) {
                if share >= &config::Share::Duplicate {
                    if let Some(cred) = get_shared_pwd_repo(repo, repo_path, &seen) {
                        return Some(cred);
                    }
                }
            }

            if share >= &config::Share::Org {
                if let Some(cred) = get_shared_pwd_org(org, repo_path, &seen) {
                    return Some(cred);
                }
            }
        }

        if share >= &config::Share::Domain {
            if let Some(cred) = get_shared_pwd_domain(domain, repo_path, &seen) {
                return Some(cred);
            }
        };
    };

    None
}

fn prompt_cred(url: String, is_ssh: bool, keys: Vec<&GitCred>) -> GitCred {
    if is_ssh { prompt_ssh(url, keys) } else { prompt_plaintext(url) }
}

fn prompt_ssh(url: String, keys: Vec<&GitCred>) -> GitCred {
    if keys.len() > 0 {
        return pick(&keys);
    }
    let ssh_path = format!("{}/.ssh/id_rsa", env::var("HOME").expect("No env var HOME present"));
    println!("\nNo verified, untried ssh keys found. Please enter password for ssh key assumed to exist at {}, for \"{}\":", &ssh_path, &url);
    let ssh_pass = config::prompt_ssh_pass(&ssh_path);
    GitCred::Ssh(ssh_path, string_ops::str_to_opt(ssh_pass))
}

fn prompt_plaintext(url: String) -> GitCred {
    let input_msg = format!("Password:");
    let confirm_msg = format!("Confirm:");
    let did_not_match_msg = format!("Passwords must match");
    let mut new_pass: String;
    let mut confirm_pass: String;
    println!("\nPlease enter plaintext password for upgitting \"{}\":", &url);
    new_pass = rpassword::read_password_from_tty(Some(&input_msg)).expect("could not access tty");
    confirm_pass = rpassword::read_password_from_tty(Some(&confirm_msg)).expect("could not access tty");
    while new_pass != confirm_pass {
        println!("{}", did_not_match_msg);
        new_pass = rpassword::read_password_from_tty(Some(&input_msg)).expect("could not access tty");
        confirm_pass = rpassword::read_password_from_tty(Some(&confirm_msg)).expect("could not access tty");
    }
    GitCred::Plain(new_pass)
}

impl Storage {
    pub fn from_config(config: &config::Config) -> Storage {
        let mut storage = Storage {
            keys: HashSet::new(),
            default_ssh: match &config.default_ssh {
                (path, pass) => {
                    if path == &String::from("") {
                        None
                    } else {
                        Some(GitCred::Ssh(path.clone(), pass.clone()))
                    }
                },
            },
            default_plain: match &config.default_plain {
                Some(x) => Some(GitCred::Plain(x.clone())),
                _ => None
            },
            share: config.share.clone(),
            repo_graph: HashMap::new(),
        };

        for (k, v) in config.plain.iter() {
            if let Ok(git_url) = parse_url(k.to_string()) {
                storage.ensure_repo_node(&git_url);
                let domain_key = url_to_domain(&git_url);
                if let Some(domain) = storage.repo_graph.get_mut(&domain_key) {
                    if let Some(org) = domain.get_mut(&git_url.org) {
                        if let Some(repo) = org.get_mut(&git_url.repo) {
                            repo.insert(
                                String::from(""),
                                RepoCred {
                                    active: GitCred::Plain(v.to_string()),
                                    seen: HashSet::new(),
                                }
                            );
                        };
                    };
                };
            }
        };

        for (k, v) in config.ssh.iter() {
            storage.keys.insert(GitCred::Ssh(k.to_string(), string_ops::str_to_opt(v.to_string())));
        }

        storage
    }

    fn get_cred(&mut self, git_url: GitUrl, repo_path: String, is_ssh: bool, url: String) -> GitCred {
        self.ensure_repo_node(&git_url);
        let domain_key = url_to_domain(&git_url);
        let rg_clone = self.repo_graph.clone();
        if let Some(domain) = self.repo_graph.get_mut(&domain_key) {
            if let Some(org) = domain.get_mut(&git_url.org) {
                if let Some(repo) = org.get_mut(&git_url.repo) {
                    match repo.get_mut(&repo_path) {
                        Some(pathed_repo) => {
                            // If this path is reached, it means the cred was previous tried
                            // and failed. So we need to get a new one. And add the old one
                            // to the set of seen creds.
                            pathed_repo.seen.insert(pathed_repo.active.clone());
                            if let Some(shared_cred) = get_shared_cred(&rg_clone, &self.share, git_url, &repo_path, &pathed_repo.seen) {
                                let cred_clone = shared_cred.clone();
                                pathed_repo.active = shared_cred;
                                return cred_clone;
                            } else {
                                if let Some(default_cred) = if is_ssh { self.default_ssh.clone() } else { self.default_plain.clone() } {
                                    if !pathed_repo.seen.contains(&default_cred) {
                                        let cred_clone = default_cred.clone();
                                        pathed_repo.active = default_cred;
                                        return cred_clone;
                                    }
                                }
                                let untried_keys: Vec<_> = self.keys.difference(&pathed_repo.seen).collect();
                                let cred = prompt_cred(url, is_ssh, untried_keys);
                                let cred_clone = cred.clone();
                                if pathed_repo.seen.contains(&cred) {
                                    println!("You already tried this cred, but trying again anyways.");
                                }
                                pathed_repo.active = cred;
                                return cred_clone;
                            }
                        },
                        None => {
                            let active = match get_shared_cred(&rg_clone, &self.share, git_url, &repo_path, &HashSet::new()) {
                                Some(cred) => cred,
                                None => {
                                    if let Some(default_cred) = if is_ssh { self.default_ssh.clone() } else { self.default_plain.clone() } {
                                        default_cred.clone()
                                    } else {
                                        prompt_cred(url, is_ssh, self.keys.iter().collect())
                                    }
                                },
                            };
                            let active_clone = active.clone();
                            repo.insert(
                                repo_path,
                                RepoCred { active, seen: HashSet::new() },
                            );
                            return active_clone;
                        },
                    };
                };
            };
        };
        panic!("This should never be reached");
    }

    fn ensure_repo_node(&mut self, git_url: &GitUrl) {
        let domain_key = url_to_domain(git_url);
        match self.repo_graph.get_mut(&domain_key) {
            Some(domain) => {
                match domain.get_mut(&git_url.org) {
                    Some(org) => {
                        if let None = org.get(&git_url.repo) {
                            org.insert(git_url.repo.clone(), HashMap::new());
                        }
                    },
                    None => {
                        let repo = HashMap::new();
                        let mut org = HashMap::new();
                        org.insert(git_url.repo.clone(), repo);
                        domain.insert(git_url.org.clone(), org);
                    },
                };
            },
            None => {
                let repo = HashMap::new();
                let mut org = HashMap::new();
                let mut domain = HashMap::new();
                org.insert(git_url.repo.clone(), repo);
                domain.insert(git_url.org.clone(), org);
                self.repo_graph.insert(domain_key, domain);
            }
        };
    }
}

fn url_to_domain(git_url: &GitUrl) -> String {
    format!("{}://{}@{}", git_url.scheme, git_url.username, git_url.domain)
}

type SharedData = std::sync::Arc<std::sync::Mutex<Storage>>;

fn git_cred_to_cred(username: String, cred: GitCred) -> Result<Cred, git2::Error> {
    match cred {
        GitCred::Ssh(path, key_pass) => {
            Cred::ssh_key(
                &username,
                None,
                Path::new(&path),
                key_pass.as_deref(),
            )
        },
        GitCred::Plain(pass) => Cred::userpass_plaintext(&username, &pass)
    }
}

fn pick(choices: &Vec<&GitCred>) -> GitCred {
    let num_choices = choices.len();
    if num_choices == 0 {
        panic!("Unable to pick value from empty list");
    };
    if num_choices == 1 {
        return choices[0].clone();
    };
    let mut i = 1;
    println!("\nPick one of the following:");
    for x in choices.iter() {
        if let GitCred::Ssh(x, _) = x {
            println!("{}) {:?}", i, x);
        }
        i += 1;
    }
    let choice: String = read!("{}\n");
    if let Ok(num) = choice.parse::<usize>() {
        if num <= num_choices {
            return choices[num].clone();
        };
    };

    println!("Please enter a number, 1 - {}", num_choices);
    pick(choices)
}

pub fn callback(url: &str, username_from_url: Option<&str>, allowed_types: CredentialType, shared_data: SharedData, repo_path: &String) -> Result<Cred, git2::Error> {
    if allowed_types.is_ssh_key() {
        let user = username_from_url.unwrap_or("git");
        let mut shared_data = shared_data.lock().expect("could not acquire lock");
        let new_cred = shared_data.get_cred(
            parse_url(String::from(url)).expect(format!("Expected url \"{}\" to parse :(", url).as_str()),
            repo_path.to_string(),
            true,
            url.to_string(),
        );
        git_cred_to_cred(user.to_string(), new_cred)
    } else if  allowed_types.is_user_pass_plaintext() {
        let user = username_from_url.expect("no username available in git url");
        let mut shared_data = shared_data.lock().expect("unable to acquire lock");
        let new_cred = shared_data.get_cred(
            parse_url(String::from(url)).expect("Expected url to parse :("),
            repo_path.to_string(),
            false,
            url.to_string(),
        );
        git_cred_to_cred(user.to_string(), new_cred)
    } else {
        Err(git2::Error::from_str("Unable to select a credential type, only plaintext or ssh key are supported at this time."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod parse_url {
        fn mk_url(domain: &str, org: &str, repo: &str, scheme: &str, username: &str) -> GitUrl {
            GitUrl{
                domain: String::from(domain),
                org: String::from(org),
                repo: String::from(repo),
                scheme: String::from(scheme),
                username: String::from(username),
            }
        }
        use super::*;
        #[test]
        fn plain_domains() {
            let input_url = String::from("https://github.com");
            let expected = mk_url("github.com", "", "", "https", "");
            assert_eq!(parse_url(input_url).unwrap(), expected);
        }

        #[test]
        fn plain_domains_trailing_slash() {
            let input_url = String::from("https://github.com/");
            let expected = mk_url("github.com", "", "", "https", "");
            assert_eq!(parse_url(input_url).unwrap(), expected);
        }

        #[test]
        fn username_domains() {
            let input_url = String::from("https://neallred@github.com/");
            let expected = mk_url("github.com", "", "", "https", "neallred");
            assert_eq!(parse_url(input_url).unwrap(), expected);
        }

        #[test]
        fn orgless_repo() {
            let input_url = String::from("https://gitstub.io/repo-sans-org.git");
            let expected = mk_url("gitstub.io", "", "repo-sans-org.git", "https", "");
            assert_eq!(parse_url(input_url).unwrap(), expected);
        }

        #[test]
        fn orgless_repo_sans_extension() {
            let input_url = String::from("https://gitstub.io/repo-sans-org");
            let expected = mk_url("gitstub.io", "", "repo-sans-org", "https", "");
            assert_eq!(parse_url(input_url).unwrap(), expected);
        }

        #[test]
        fn short_orgs() {
            let input_url = String::from("https://github.com/org/repo.git");
            let expected = mk_url("github.com", "org", "repo.git", "https", "");
            assert_eq!(parse_url(input_url).unwrap(), expected);
        }

        #[test]
        fn long_orgs() {
            let input_url = String::from("https://gitstub.io/my/long/org/repo.git");
            let expected = mk_url("gitstub.io", "my/long/org", "repo.git", "https", "");
            assert_eq!(parse_url(input_url).unwrap(), expected);
        }

        #[test]
        fn git_ssh_urls() {
            let input_url = String::from("git@gitstub.io/my/long/org/repo.git");
            let expected = mk_url("gitstub.io", "my/long/org", "repo.git", "unknown", "git");
            assert_eq!(parse_url(input_url).unwrap(), expected);
        }
    }

    mod ensure_repo_node {
        use super::*;
        #[test]
        fn insert_when_blank() {
            let mut storage = Storage {
                repo_graph: HashMap::new(),
                default_ssh: None,
                default_plain: None,
                share: config::Share::Never,
                keys: HashSet::new(),
            };

            let url = String::from("git@gitstub.io/org/repo.git");
            let git_url = parse_url(url).unwrap();
            storage.ensure_repo_node(&git_url);
            // panics (fails test) if does not exist
            &storage.repo_graph[&String::from("unknown://git@gitstub.io")][&String::from("org")][&String::from("repo.git")];
        }
    }
}
