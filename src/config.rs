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
            response = rpassword::read_password_from_tty(Some(&prompt)).unwrap();
        } else {
            print!("{}" , prompt);
            io::stdout().flush().unwrap();
            response = read!("{}\n");
        }
        if sensitive {
            response_confirm = rpassword::read_password_from_tty(Some(&format!("Confirm:"))).unwrap();
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

fn get_git_dirs(matches: &ArgMatches) -> Vec<String> {
    let git_dirs_args: Vec<_> = matches.values_of("git-dirs").unwrap_or_default().map(|x| String::from(x)).collect();
    if git_dirs_args.len() > 0 {
        return git_dirs_args
    }

    if let Ok(string) = env::var("UPGIT_GIT_DIRS") {
        let git_dirs: Vec<_> = string.split(",").map(|x| x.to_string()).collect();
        if git_dirs.len() > 0 {
            return git_dirs;
        }
    }

    println!("Git directories were not provided via $UPGIT_GIT_DIRS, CLI, or config file. Provide space separated list via stdin:");
    let git_dirs_str: String = read!("{}\n");
    if git_dirs_str.len() == 0 {
        println!("No git directories provided, exiting");
        std::process::exit(1);
    }

    let git_dirs: Vec<_> = git_dirs_str.split(" ").map(|x| {
        let path_str: String = shellexpand::tilde(x).into_owned();
        std::fs::canonicalize(&path_str)
            .or_else(|_| std::fs::read_link(x))
            .expect(&format!("\"{}\" was not a canonical path or symlink", path_str))
            .to_str()
            .expect(&format!("\"{}\"was not a path", path_str))
            .to_string()
    }).collect();

    git_dirs 
}

fn get_default_ssh(matches: &ArgMatches) -> Option<String> {
    if let Some(_) = matches.value_of("default-ssh") {
        let response = prompt_confirm(format!("Enter default ssh key pass (blank for none): "), false, true);
        return match response {
            pwd if pwd == String::from("") => None,
            pwd => Some(pwd),
        };
    };
    None
}

fn get_default_plain(matches: &ArgMatches) -> Option<String> {
    if matches.is_present("default-plain") {
        let response = prompt_confirm(format!("Enter default plaintext authentication method pass (blank for none): "), false, true);
        return match response {
            pwd if pwd == String::from("") => None,
            pwd => Some(pwd),
        };
    };
    None
}

fn get_ssh_keys(matches: &ArgMatches) -> HashMap<String, String> {
    let ssh_keys: HashMap<String, String> = matches.values_of("ssh").unwrap_or_default().map(|ssh_key_path| {(
            ssh_key_path.to_string(),
            prompt_ssh_pass(ssh_key_path.to_string()),
    )}).collect();
    return ssh_keys
}

fn get_plaintexts(matches: &ArgMatches) -> HashMap<String, String> {
    let ssh_keys: HashMap<String, String> = matches.values_of("plain").unwrap_or_default().map(|user_url| {(
            user_url.to_string(),
            prompt_confirm(format!("enter password for url \"{}\" (required): ", user_url), true, true),
    )}).collect();
    return ssh_keys
}

pub fn new() -> Config {
    let matches = App::new("upgit")
        .version("0.1.0")
        .author("Nathaniel Allred <neallred@gmail.com>")
        .about("Pulls all repos within a folder containing git projects, in parallel. Supports configuration via command line flags and params. Partial ENV var support. Command line takes precedence. If no option is set but is needed (i.e. repos requiring auth), the program will prompt if a TTY is available, or skip that repo if it is not available.")
        .arg(
            Arg::with_name("plain")
            .long("plain")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .long_help("Git repo https url with username. For example, `--plain https://neallred@bitbucket.org/neallred/allredlib-data-backup.git`. For each time this option is passed, user will be prompted for a password.")
        )
        .arg(
            Arg::with_name("ssh")
            .long("ssh")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .long_help("Paths to ssh keys to preverify. User will be prompted for password for each key given. Can enter \"blank\" if ssh key is not password protected")
        )
        .arg(
            Arg::with_name("default-plain")
            .long("default-plain")
            .long_help("Default password to attempt for https cloned repos. User will be prompted for the password")
            .takes_value(false)
        )
        .arg(
            Arg::with_name("default-ssh")
            .long("default-ssh")
            .long_help("Default password to use for ssh keys. User will be prompted for the password")
        )
        .arg(
            Arg::with_name("git-dirs")
            .index(1)
            .multiple(true)
            .long_help("Paths to folders that contain git repos. Can be relative or absolute. Overrides a (comma separated) UPGIT_GIT_DIRS env var. One or the other must be set.")
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
