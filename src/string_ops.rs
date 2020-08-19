pub fn str_to_opt(x: String) -> Option<String> {
    if x == String::from("") {
        None
    } else {
        Some(x)
    }
}
