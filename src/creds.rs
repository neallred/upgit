use git2::{Cred, CredentialType};
use std::path::Path;
use rpassword;
use std::collections::HashMap;
use crate::config;
use std::env;

// TODO Ideas for improving authing to repos:
// * Maintain an in-memory data structure of already attempted passwords for specific domains

#[derive(Debug, Clone)]
pub struct Storage {
    default_ssh: String,
    default_plain: String,
    domain_plaintext: HashMap<String, String>
}

impl Storage {
    pub fn from_config(config: &config::Config) -> Storage {
        Storage {
            default_ssh: match &config.default_ssh {
                Some(x) => x.clone(),
                _ => String::from(""),
            },
            default_plain: match &config.default_plain {
                Some(x) => x.clone(),
                _ => String::from(""),
            },
            domain_plaintext: HashMap::new(),
        }
    }

    fn get_ssh_passphrase(&mut self, user: String, remote_url: String) -> String {
        if self.default_ssh == String::from("") {
            self.default_ssh = rpassword::read_password_from_tty(Some(&format!("\nEnter passphrase for private key $HOME/.ssh/id_rsa (or enter for blank):\n"))).unwrap();
        }
        self.default_ssh.clone()
    }

    fn get_plaintext(&mut self, user: String, remote_url: String) -> String {
        // This simplistic data structure and flow is geared to
        // assuming a single user account for most of the repos.
        // It can be refined if many users are using many accounts.

        let input_msg = format!("Password:");
        let confirm_msg = format!("Confirm:");
        let did_not_match_msg = format!("Passwords must match");
        let existing_default = self.default_plain.clone();
        let pw_entry = self.domain_plaintext.entry(remote_url.clone())
            .and_modify(|password_to_fix| {
                // assume that if hitting the same url,
                // its not that they have duplicate repos,
                // but that they keyed a password wrong.
                // real tracking would be more involved.
                let mut new_pass: String;
                let mut confirm_pass: String;
                println!("\nAuthenticating user \"{}\" at \"{}\":", &user, &remote_url);
                new_pass = rpassword::read_password_from_tty(Some(&input_msg)).unwrap();
                confirm_pass = rpassword::read_password_from_tty(Some(&confirm_msg)).unwrap();
                while new_pass != confirm_pass {
                    println!("{}", did_not_match_msg);
                    new_pass = rpassword::read_password_from_tty(Some(&input_msg)).unwrap();
                    confirm_pass = rpassword::read_password_from_tty(Some(&confirm_msg)).unwrap();
                }
                *password_to_fix = new_pass;
            })
            .or_insert_with(|| {
                if existing_default == String::from("") {
                    println!("\nAuthenticating user \"{}\" at \"{}\":", &user, &remote_url);
                    let mut new_pass: String;
                    let mut confirm_pass: String;
                    new_pass = rpassword::read_password_from_tty(Some(&input_msg)).unwrap();
                    confirm_pass = rpassword::read_password_from_tty(Some(&confirm_msg)).unwrap();
                    while new_pass != confirm_pass {
                        println!("{}", did_not_match_msg);
                        new_pass = rpassword::read_password_from_tty(Some(&input_msg)).unwrap();
                        confirm_pass = rpassword::read_password_from_tty(Some(&confirm_msg)).unwrap();
                    }
                    new_pass.clone()
                } else {
                    existing_default
                }
            });
        self.default_plain = pw_entry.clone();
        pw_entry.clone()
    }
}

type SharedData = std::sync::Arc<std::sync::Mutex<Storage>>;
pub fn callback(url: &str, username_from_url: Option<&str>, allowed_types: CredentialType, shared_data: SharedData) -> Result<Cred, git2::Error> {
    if allowed_types.is_ssh_key() {
        let mut shared_data = shared_data.lock().unwrap();

        let user = username_from_url.unwrap();
        let pass = shared_data.get_ssh_passphrase(user.to_string(), url.to_string());
        Cred::ssh_key(
            username_from_url.unwrap(),
            Some(Path::new(&format!("{}/.ssh/id_rsa.pub", env::var("HOME").unwrap()))),
            Path::new(&format!("{}/.ssh/id_rsa", env::var("HOME").unwrap())),
            if pass == String::from("") { None } else { Some(&pass) },
        )
    } else if  allowed_types.is_user_pass_plaintext() {
        let user = username_from_url.unwrap();
        let mut shared_data = shared_data.lock().unwrap();
        let pass = shared_data.get_plaintext(user.to_string(), url.to_string());
        Cred::userpass_plaintext(user, &pass)
    } else {
        Err(git2::Error::from_str("Unable to select a credential type, only plaintext or ssh key are supported at this time."))
    }
}
