use rpassword;
use std::collections::HashMap;

// TODO Ideas for improving authing to repos:
// * Allow user to be prompted for a default plaintext password as command line flag
// * Allow user to be prompted for a default ssh passphrase as command line flag
// * Maintain an in-memory data structure of already attempted passwords for specific domains

#[derive(Debug, Clone)]
pub struct Storage {
    default_ssh_passphrase: String,
    default_plaintext: String,
    domain_plaintext: HashMap<String, String>
}

impl Storage {
    pub fn blank() -> Storage {
        Storage {
            default_ssh_passphrase: String::from(""),
            default_plaintext: String::from(""),
            domain_plaintext: HashMap::new(),
        }
    }

    pub fn get_ssh_passphrase(&mut self) -> String {
        if self.default_ssh_passphrase == String::from("") {
            self.default_ssh_passphrase = rpassword::read_password_from_tty(Some(&format!("\nEnter passphrase for private key $HOME/.ssh/id_rsa (or enter for blank):\n"))).unwrap();
        }
        self.default_ssh_passphrase.clone()
    }

    pub fn get_plaintext(&mut self, user: String, remote_url: String) -> String {
        // This simplistic data structure and flow is geared to
        // assuming a single user account for most of the repos.
        // It can be refined if many users are using many accounts.

        let input_msg = format!("Password:");
        let confirm_msg = format!("Confirm:");
        let did_not_match_msg = format!("Passwords must match");
        let existing_default = self.default_plaintext.clone();
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
        self.default_plaintext = pw_entry.clone();
        pw_entry.clone()
    }
}
