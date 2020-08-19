use clap::{Arg, App, ArgMatches};
use std::env;
use text_io::read;
use shellexpand;
use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use rpassword;
use std::process::Command;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use crate::string_ops;

#[derive(Debug)]
pub struct Config {
    pub ssh: HashMap<String, String>, // path, pass
    pub plain: HashMap<String, String>,
    pub default_plain: Option<String>,
    pub default_ssh: (String, Option<String>),
    pub git_dirs: Vec<String>,
    pub share: Share,
}

fn prompt_confirm(prompt: String, required: bool, sensitive: bool) -> String {
    let mut response = String::from("a");
    let mut response_confirm = String::from("b");
    while response != response_confirm {
        if sensitive {
            response = rpassword::read_password_from_tty(Some(&prompt)).expect("Unable to read password from tty");
        } else {
            print!("{}" , prompt);
            match io::stdout().flush() {
                Ok(_) => {},
                _ => {}
            };
            response = read!("{}\n");
        }
        if sensitive {
            response_confirm = rpassword::read_password_from_tty(Some(&format!("Confirm:"))).expect("Unable to read password from tty");
        } else {
            print!("Confirm: ");
            match io::stdout().flush() {
                Ok(_) => {},
                _ => {}
            };
            response_confirm = read!("{}\n");
        }
    };
    if required && response == String::from("") {
        println!("\n Info required.");
        return prompt_confirm(prompt, required, sensitive)
    }
    response
}

enum SshVerify {
    Dunno,
    Good,
    Bad,
}

#[derive(Debug, Clone, PartialEq,  Eq, PartialOrd, Ord)]
pub enum Share {
    Never,
    Defaults,
    Duplicate,
    Org,
    Domain,
}

fn verify_ssh_pass(private_key_path: &String, passphrase: &String) -> SshVerify {
    let tmp_path = "/tmp/echo-upgit-pw";
    let tmp_var = "TMP_UPGIT_PW";
    match File::create(tmp_path) {
        Ok(mut file) => {
            match file.write_all(format!("#!/usr/bin/env sh\necho ${}\n", tmp_var).as_bytes()) {
                Ok(_) => {
                    match file.metadata() {
                        Ok(metadata) => {
                            let mut permissions = metadata.permissions();
                            permissions.set_mode(0o700);
                            match std::fs::set_permissions(tmp_path, permissions) {
                                Ok(_) => {},
                                Err(_) => return SshVerify::Dunno,
                            }
                        }
                        Err(_) => return SshVerify::Dunno,
                    }
                },
                Err(_) => return SshVerify::Dunno,
            };
        }
        Err(_) => return SshVerify::Dunno,
    };

    let cmd = Command::new("ssh-keygen")
        .arg("-y")
        .arg("-f")
        .arg(private_key_path)
        .env(tmp_var, passphrase)
        .env("SSH_ASKPASS", tmp_path)
        .output();

    if let Ok(output) = cmd {
        if let Ok(stdout) = std::str::from_utf8(&output.stdout) {
            if stdout.len() > 0 && output.status.success() {
                return SshVerify::Good;
            }
            return SshVerify::Bad;
        };
    };
    SshVerify::Good
}

pub fn prompt_ssh_pass(private_key_path: &String) -> String {
    let response = match rpassword::read_password_from_tty(Some(&format!("Enter password for ssh key {} (blank for none): ", private_key_path))) {
        Ok(x) => x,
        // If there is no tty, for example in e2e tests,
        // we should at least allow a potentially valid no password scenario
        _ => String::from(""),
    };
    match verify_ssh_pass(&private_key_path, &response) {
        SshVerify::Good => return response,
        SshVerify::Dunno => {
            print!("Confirm: ");
            match io::stdout().flush() {
                Ok(_) => {},
                _ => {}
            };
            let response_confirm: String = read!("{}\n");
            if response == response_confirm {
                return response
            }
            return prompt_ssh_pass(private_key_path);
        },
        SshVerify::Bad => return prompt_ssh_pass(private_key_path),
    }
}

fn relative_to_absolute_path(x: &str) -> String {
    let path_str: String = shellexpand::tilde(x).into_owned();
    std::fs::canonicalize(&path_str)
        .or_else(|_| std::fs::read_link(x))
        .expect(&format!("\"{}\" was not a canonical path or symlink", path_str))
        .to_str()
        .expect(&format!("\"{}\"was not a path", path_str))
        .to_string()
}

fn get_git_dirs(matches: &ArgMatches) -> Vec<String> {
    let git_dirs_args: Vec<_> = matches.values_of("git-dirs").unwrap_or_default().map(|x| String::from(x)).collect();
    if git_dirs_args.len() > 0 {
        return git_dirs_args
    }

    if let Ok(string) = env::var("UPGIT_GIT_DIRS") {
        let git_dirs: Vec<_> = string.split(",").map(relative_to_absolute_path).collect();
        if git_dirs.len() > 0 {
            return git_dirs;
        }
    }

    println!("Git directories were not provided via $UPGIT_GIT_DIRS or CLI. Provide space separated list via stdin:");
    let git_dirs_str: String = read!("{}\n");
    if git_dirs_str.len() == 0 {
        println!("No git directories provided, exiting");
        std::process::exit(1);
    }

    let git_dirs: Vec<_> = git_dirs_str.split(" ").map(relative_to_absolute_path).collect();

    git_dirs 
}

fn get_default_ssh(matches: &ArgMatches) -> (String, Option<String>) {
    let key_path = match matches.value_of("default-ssh") {
        Some(path) => {
            if path == String::from("") {
                format!("{}/.ssh/id_rsa", env::var("HOME").expect("Unable to find HOME env var"))
            } else {
                path.to_string()
            }
        },
        None => format!(""),
    };

    let key_pass = if matches.is_present("default-ssh") || env::var("UPGIT_DEFAULT_SSH").is_ok() {

        let response = prompt_confirm(format!("Enter default ssh key pass (blank for none): "), false, true);
        string_ops::str_to_opt(response)
    } else {
        None
    };

    (key_path, key_pass)
}

fn get_default_plain(matches: &ArgMatches) -> Option<String> {
    if matches.is_present("default-plain") || env::var("UPGIT_DEFAULT_PLAIN").is_ok() {
        return Some(prompt_confirm(format!("Enter default plaintext authentication method pass (blank for none): "), true, true));
    };

    None
}

fn str_to_share(x: &str) -> Share {
    if x == "none" { Share::Never }
    else if x == "default" { Share::Defaults }
    else if x == "duplicate" { Share::Duplicate }
    else if x == "org" { Share::Org }
    else if x == "domain" { Share::Domain }
    else { Share::Defaults }
}

fn get_share(matches: &ArgMatches) -> Share {
    if let Some(share_str) = matches.value_of("share") {
        return str_to_share(&share_str);
    }

    if let Ok(share_str) = env::var("UPGIT_SHARE") {
        return str_to_share(&share_str);
    }

    Share::Defaults
}

fn get_ssh_keys(matches: &ArgMatches) -> HashMap<String, String> {
    if let Some(key_paths) = matches.values_of("ssh") {
        return key_paths.map(|path| {(
            path.to_string(),
            prompt_ssh_pass(&path.to_string()),
        )}).collect();
    };

    if let Ok(string) = env::var("UPGIT_SSH") {
        return string.split(",").map(|path| {(
            path.to_string(),
            prompt_ssh_pass(&path.to_string()),
        )}).collect();
    }

    HashMap::new()
}

fn get_plaintexts(matches: &ArgMatches) -> HashMap<String, String> {
    if let Some(user_urls) = matches.values_of("plain") {
        return user_urls.map(|user_url| {(
            user_url.to_string(),
            prompt_confirm(format!("enter password for url \"{}\" (required): ", user_url), true, true),
        )}).collect();
    }

    if let Ok(string) = env::var("UPGIT_PLAIN") {
        return string.split(",").map(|path| {(
            path.to_string(),
            prompt_confirm(format!("enter password for url \"{}\" (required): ", path.to_string()),true, true),
        )}).collect();
    }

    HashMap::new()
}

pub fn new() -> Config {
    let matches = App::new("upgit")
        .version("0.1.0")
        .author("Nathaniel Allred <neallred@gmail.com>")
        .about("Updates repos in a folder containing git projects, in parallel. Supports configuration via command line flags and params, and via ENV vars. Command line takes precedence. If no option is set but is needed (i.e. repos requiring auth), user will be prompted if a TTY is available, otherwise the process will exit unsuccessfully.")
        .set_term_width(80)
        .arg(
            Arg::with_name("plain")
            .long("plain")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .long_help("Git repo https url with username. For example, `--plain https://neallred@bitbucket.org/neallred/allredlib-data-backup.git`. For each time this option is passed, user will be prompted for a password. Env var is comma separated UPGIT_PLAIN.")
        )
        .arg(
            Arg::with_name("ssh")
            .long("ssh")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .long_help("Paths to ssh keys to preverify. User will be prompted for password for each key given. Can enter empty password if key has no password. Env var is comma separated UPGIT_SSH.")
        )
        .arg(
            Arg::with_name("default-plain")
            .long("default-plain")
            .long_help("Default password to attempt for http(s) cloned repos. User will be prompted for the password. Env var is UPGIT_DEFAULT_PLAIN set to any value, including empty.")
            .takes_value(false)
        )
        .arg(
            Arg::with_name("default-ssh")
            .long("default-ssh")
            .long_help("Default password to use for ssh keys. User will be prompted for the password. Env var is UPGIT_DEFAULT_SSH set to path of key (or empty, in which case $HOME/.ssh/id_rsa is assumed).")
        )
        .arg(
            Arg::with_name("share")
            .long("share")
            .takes_value(true)
            .possible_values(&["none", "default", "duplicate", "org", "domain"])
            .default_value("default")
            .long_help("Degree to which credentials may reused between repos needing auth. Each level is additive. `none` means no credential reuse between repos, and defaults are ignored. `default` means default provided credentials may be reused. `duplicate` means defaults, plus multiple copies of a repo can reuse each other's credential. `org` means duplicate, plus upgit will infer a matching org by looking at the second to last url path segment (e.g. `neallred` in https://github.com/neallred/upgit`). `domain` means reusing when user and url domain match. Env var is UPGIT_SHARE.")
        )
        .arg(
            Arg::with_name("git-dirs")
            .index(1)
            .multiple(true)
            .long_help("Paths (relative or absolute) to folders that contain git repos. Env var is comma separated UPGIT_GIT_DIRS.")
        )
        .get_matches();

    let config = Config {
        ssh: get_ssh_keys(&matches),
        plain: get_plaintexts(&matches),
        default_ssh: get_default_ssh(&matches),
        default_plain: get_default_plain(&matches),
        git_dirs: get_git_dirs(&matches),
        share: get_share(&matches),
    };

    config
}
