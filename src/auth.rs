use anyhow::{Result, anyhow, bail};
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::SaltString,
};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};

pub fn hash_password(password: &str) -> Result<String> {
    if password.len() < 6 || password.len() > 256 {
        bail!("Password must contain 6–256 bytes");
    }
    let params = Params::new(19_456, 2, 1, None).map_err(|e| anyhow!(e.to_string()))?;
    let salt = SaltString::generate(&mut OsRng);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password(password.as_bytes(), &salt)
        .map(|s| s.to_string())
        .map_err(|e| anyhow!(e.to_string()))
}
pub fn verify(password: &str, hash: &str) -> bool {
    if password.len() > 256 {
        return false;
    }
    PasswordHash::new(hash).ok().is_some_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}
pub fn token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn digest(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
