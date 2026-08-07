use crate::util::hash::md5_hex;
use rand::{distributions::Alphanumeric, Rng};

pub fn random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub fn gen_encrypted_password(password: &str, salt: &str) -> String {
    let first = md5_hex(&format!("{}{}", password, salt));
    md5_hex(&format!("{}{}", first, salt))
}
