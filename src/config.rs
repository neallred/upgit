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

#[derive(Debug)]
pub struct Config {
    pub ssh: HashMap<String, String>,
    pub plain: HashMap<String, String>,
    pub default_plain: Option<String>,
    pub default_ssh: Option<String>,
    pub git_dirs: Vec<String>,
}

fn prompt_confirm(prompt: String, required: bool, sensitive: bool) -> String {
    let mut response = String::from("a");
    let mut response_confirm = String::from("b");
    while response != response_confirm {
        if sensitive {
            response = rpassword::read_password_from_tty(Some(&prompt)).expect("Unable to read password from tty");
        } else {
            print!("{}" , prompt);
            io::stdout().flush().unwrap();
            response = read!("{}\n");
        }
        if sensitive {
            response_confirm = rpassword::read_password_from_tty(Some(&format!("Confirm:"))).expect("Unable to read password from tty");
        } else {
            print!("Confirm: ");
            io::stdout().flush().unwrap();
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

fn prompt_ssh_pass(private_key_path: String) -> String {
    let response = rpassword::read_password_from_tty(Some(&format!("Enter password for ssh key {} (blank for none): ", private_key_path))).unwrap();
    match verify_ssh_pass(&private_key_path, &response) {
        SshVerify::Good => return response,
        SshVerify::Dunno => {
            print!("Confirm: ");
            io::stdout().flush().unwrap();
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

fn get_default_ssh(matches: &ArgMatches) -> Option<String> {
    if matches.is_present("default-ssh") || env::var("UPGIT_DEFAULT_SSH").is_ok() {
        let response = prompt_confirm(format!("Enter default ssh key pass (blank for none): "), false, true);
        return match response {
            pwd if pwd == String::from("") => None,
            pwd => Some(pwd),
        };
    };

    None
}

fn get_default_plain(matches: &ArgMatches) -> Option<String> {
    if matches.is_present("default-plain") || env::var("UPGIT_DEFAULT_PLAIN").is_ok() {
        return Some(prompt_confirm(format!("Enter default plaintext authentication method pass (blank for none): "), true, true));
    };

    None
}

fn get_ssh_keys(matches: &ArgMatches) -> HashMap<String, String> {
    if let Some(key_paths) = matches.values_of("ssh") {
        return key_paths.map(|path| {(
            path.to_string(),
            prompt_ssh_pass(path.to_string()),
        )}).collect();
    };

    if let Ok(string) = env::var("UPGIT_SSH") {
        return string.split(",").map(|path| {(
            path.to_string(),
            prompt_ssh_pass(path.to_string()),
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
            prompt_ssh_pass(path.to_string()),
        )}).collect();
    }

    HashMap::new()
}

pub fn new() -> Config {
    let matches = App::new("upgit")
        .version("0.1.0")
        .author("Nathaniel Allred <neallred@gmail.com>")
        .about("Pulls all repos within a folder containing git projects, in parallel. Supports configuration via command line flags and params, or via ENV vars. Command line takes precedence. If no option is set but is needed (i.e. repos requiring auth), user will be prompted if a TTY is available, or skip that repo if it is not available.")
        .arg(
            Arg::with_name("plain")
            .long("plain")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .long_help("Git repo https url with username. For example, `--plain https://neallred@bitbucket.org/neallred/allredlib-data-backup.git`. For each time this option is passed, user will be prompted for a password. Env var is comma separated UPGIT_PLAIN")
        )
        .arg(
            Arg::with_name("ssh")
            .long("ssh")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .long_help("Paths to ssh keys to preverify. User will be prompted for password for each key given. Can enter \"blank\" if ssh key is not password protected. Env var is comma separated UPGIT_SSH")
        )
        .arg(
            Arg::with_name("default-plain")
            .long("default-plain")
            .long_help("Default password to attempt for https cloned repos. User will be prompted for the password. Env var is UPGIT_DEFAULT_PLAIN set to any value")
            .takes_value(false)
        )
        .arg(
            Arg::with_name("default-ssh")
            .long("default-ssh")
            .long_help("Default password to use for ssh keys. User will be prompted for the password. Env var is UPGIT_DEFAULT_SSH set to any value")
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
    };

    config
}
