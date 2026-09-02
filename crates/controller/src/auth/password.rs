use std::fs;
use std::path::{Path, PathBuf};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand_core::OsRng;
use sha2::{Digest, Sha256};

use crate::domain::SecretString;
use crate::persistence::AuthDigest;

use super::AuthError;

pub struct PasswordFile {
    path: PathBuf,
}

pub struct LoadedPasswordHash {
    encoded: String,
    fingerprint: AuthDigest,
}

impl PasswordFile {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, AuthError> {
        let store = Self {
            path: path.as_ref().to_path_buf(),
        };
        store.load()?;
        Ok(store)
    }

    pub fn load(&self) -> Result<LoadedPasswordHash, AuthError> {
        let encoded = fs::read_to_string(&self.path).map_err(|source| AuthError::PasswordFile {
            path: self.path.clone(),
            source,
        })?;
        let encoded = encoded.trim().to_owned();
        let parsed = PasswordHash::new(&encoded).map_err(|_| AuthError::InvalidPasswordHash)?;
        if parsed.algorithm.as_str() != "argon2id" {
            return Err(AuthError::InvalidPasswordHash);
        }
        Ok(LoadedPasswordHash {
            fingerprint: digest(encoded.as_bytes()),
            encoded,
        })
    }
}

impl LoadedPasswordHash {
    pub const fn fingerprint(&self) -> AuthDigest {
        self.fingerprint
    }

    pub fn verify(&self, password: &SecretString) -> bool {
        PasswordHash::new(&self.encoded).is_ok_and(|parsed| {
            Argon2::default()
                .verify_password(password.expose().as_bytes(), &parsed)
                .is_ok()
        })
    }
}

/// Hashes a raw administrator password as an Argon2id PHC string.
///
/// # Errors
/// Returns [`AuthError::PasswordHashing`] when secure hash generation fails.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::PasswordHashing)
}

pub fn digest(bytes: &[u8]) -> AuthDigest {
    AuthDigest::new(Sha256::digest(bytes).into())
}
