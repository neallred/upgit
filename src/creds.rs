use rpassword;
use std::collections::HashMap;

// TODO Ideas for improving authing to repos:
// * Pause output from threads that can continue processing because they do not need auth.
// * Allow user to be prompted for a default plaintext password as command line flag
// * Allow user to be prompted for a default ssh passphrase as command line flag
// * Maintain an in-memory data structure of already attempted passwords for specific domains

#[derive(Debug, Clone)]
pub struct Storage {
    default_ssh_passphrase: String,
    default_plaintext: String,
    tries_plaintext: HashMap<String, (String, u32)>
}

impl Storage {
    pub fn blank() -> Storage {
        Storage {
            default_ssh_passphrase: String::from(""),
            default_plaintext: String::from(""),
            tries_plaintext: HashMap::new(),
        }
    }

    pub fn get_ssh_passphrase(&mut self) -> String {
        if self.default_ssh_passphrase == String::from("") {
            self.default_ssh_passphrase = rpassword::read_password_from_tty(Some(&format!("\nEnter passphrase for private key $HOME/.ssh/id_rsa (or enter for blank):\n"))).unwrap();
        }
        self.default_ssh_passphrase.clone()
    }

    pub fn get_plaintext(&mut self, user: String, url: String) -> String {
        if self.default_plaintext == String::from("") {
            self.default_plaintext = rpassword::read_password_from_tty(Some(&format!("\nEnter password for user \"{}\" for url \"{}\":\n", user, url))).unwrap();
        }

        self.default_plaintext.clone()
    }
}
