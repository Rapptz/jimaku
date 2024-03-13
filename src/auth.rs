use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);
    Ok(argon2.hash_password(password.as_bytes(), &salt)?.to_string())
}

pub fn validate_password(password: &str, password_hash: &str) -> anyhow::Result<()> {
    let argon2 = Argon2::default();
    let hash = PasswordHash::new(password_hash)?;
    argon2.verify_password(password.as_bytes(), &hash)?;
    Ok(())
}
