use rpassword;
use std::collections::HashMap;

// TODO Ideas for improving authing to repos:
// * Pause output from threads that can continue processing because they do not need auth.
// * Allow user to be prompted for a default plaintext password as command line flag
// * Allow user to be prompted for a default ssh passphrase as command line flag
// * Maintain an in-memory data structure of already attempted passwords for specific domains

#[derive(Debug, Clone)]
pub struct Storage {
    pub ssh_pass: String,
    default_plaintext: String,
    tries_plaintext: HashMap<String, (String, u32)>
}

impl Storage {
    pub fn blank() -> Storage {
        Storage {
            ssh_pass: String::from(""),
            default_plaintext: String::from(""),
            tries_plaintext: HashMap::new(),
        }
    }

    pub fn get_plaintext(&mut self, user: String, url: String) -> String {
        // TODO
        let pass: String;
        if self.default_plaintext != String::from("") {
            return self.default_plaintext.clone();
        } else {
            pass = rpassword::read_password_from_tty(Some(&format!("\nEnter password for user \"{}\" for url \"{}\":\n\n", user, url))).unwrap();
            self.default_plaintext = pass.clone();
        }

        self.default_plaintext.clone()
    }
}
