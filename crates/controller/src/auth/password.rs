use std::sync::Arc;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use tokio::sync::{RwLock, Semaphore};

use crate::domain::SecretString;
use crate::persistence::{AuthDigest, PasswordHashRecord, Store};

use super::AuthError;

const MAX_CONCURRENT_PASSWORD_TASKS: usize = 2;

#[derive(Clone)]
pub struct PasswordEngine {
    cache: Arc<RwLock<Option<Arc<LoadedPasswordHash>>>>,
    tasks: Arc<Semaphore>,
}

pub struct LoadedPasswordHash {
    encoded: String,
    fingerprint: AuthDigest,
}

impl Default for PasswordEngine {
    fn default() -> Self {
        Self {
            cache: Arc::new(RwLock::new(None)),
            tasks: Arc::new(Semaphore::new(MAX_CONCURRENT_PASSWORD_TASKS)),
        }
    }
}

impl PasswordEngine {
    pub async fn load(&self, store: &Store) -> Result<Option<Arc<LoadedPasswordHash>>, AuthError> {
        let Some(stored) = store.administrator_credential().await? else {
            *self.cache.write().await = None;
            return Ok(None);
        };
        if let Some(cached) = self.cache.read().await.as_ref() {
            if cached.encoded() == stored.expose() {
                return Ok(Some(Arc::clone(cached)));
            }
        }
        let loaded = Arc::new(LoadedPasswordHash::parse(&stored)?);
        *self.cache.write().await = Some(Arc::clone(&loaded));
        Ok(Some(loaded))
    }

    pub async fn hash(&self, password: SecretString) -> Result<Arc<LoadedPasswordHash>, AuthError> {
        let permit = Arc::clone(&self.tasks)
            .acquire_owned()
            .await
            .map_err(|_| AuthError::PasswordVerification)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let encoded = hash_password(password.expose())?;
            LoadedPasswordHash::parse_encoded(encoded).map(Arc::new)
        })
        .await
        .map_err(|_| AuthError::PasswordVerification)?
    }

    pub async fn verify(
        &self,
        loaded: Arc<LoadedPasswordHash>,
        password: SecretString,
    ) -> Result<bool, AuthError> {
        let permit = Arc::clone(&self.tasks)
            .acquire_owned()
            .await
            .map_err(|_| AuthError::PasswordVerification)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            loaded.verify(&password)
        })
        .await
        .map_err(|_| AuthError::PasswordVerification)
    }

    pub async fn cache(&self, loaded: Arc<LoadedPasswordHash>) {
        *self.cache.write().await = Some(loaded);
    }
}

impl LoadedPasswordHash {
    fn parse(stored: &PasswordHashRecord) -> Result<Self, AuthError> {
        Self::parse_encoded(stored.expose().to_owned())
    }

    fn parse_encoded(encoded: String) -> Result<Self, AuthError> {
        let parsed = PasswordHash::new(&encoded).map_err(|_| AuthError::InvalidPasswordHash)?;
        if parsed.algorithm.as_str() != "argon2id" {
            return Err(AuthError::InvalidPasswordHash);
        }
        Ok(Self {
            fingerprint: digest(encoded.as_bytes()),
            encoded,
        })
    }

    pub const fn fingerprint(&self) -> AuthDigest {
        self.fingerprint
    }

    pub fn encoded(&self) -> &str {
        &self.encoded
    }

    fn verify(&self, password: &SecretString) -> bool {
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
