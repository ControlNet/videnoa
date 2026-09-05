use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Duration, Utc};

use crate::config::AuthConfig;
use crate::domain::{LoginResponse, SecretString};
use crate::persistence::{PersistenceError, Store};

use super::credentials::PasswordEngine;
use super::login_attempts::LoginLimiter;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("authentication failed")]
    Unauthorized,
    #[error("request proof is invalid")]
    Forbidden,
    #[error("authentication setup request is invalid")]
    InvalidRequest,
    #[error("administrator credential is already initialized")]
    Conflict,
    #[error("too many login attempts")]
    RateLimited,
    #[error("password hash is not a valid Argon2id PHC string")]
    InvalidPasswordHash,
    #[error("password hashing failed")]
    PasswordHashing,
    #[error("password verification task failed")]
    PasswordVerification,
    #[error("session lifetime cannot be represented")]
    InvalidLifetime,
    #[error("authentication persistence failed")]
    Persistence(#[from] PersistenceError),
}

#[derive(Clone)]
pub struct AuthService {
    pub(super) inner: Arc<AuthServiceInner>,
}

pub(super) struct AuthServiceInner {
    pub(super) policy: RwLock<AuthConfig>,
    pub(super) password: PasswordEngine,
    pub(super) store: Store,
    pub(super) limiter: LoginLimiter,
}

pub struct IssuedSession {
    pub response: LoginResponse,
    pub token: String,
    pub csrf: String,
}

impl AuthService {
    /// Creates an authentication service without requiring a configured credential.
    ///
    /// # Errors
    /// Returns an error when configured session lifetimes cannot be represented.
    pub fn new(config: AuthConfig, store: Store) -> Result<Self, AuthError> {
        duration(config.session_absolute)?;
        duration(config.session_idle)?;
        Ok(Self {
            inner: Arc::new(AuthServiceInner {
                policy: RwLock::new(config),
                password: PasswordEngine::default(),
                store,
                limiter: LoginLimiter::default(),
            }),
        })
    }

    /// Replaces the live cookie and session lifetime policy.
    ///
    /// # Errors
    /// Returns an error when configured session lifetimes cannot be represented.
    pub fn reconfigure(&self, config: AuthConfig) -> Result<(), AuthError> {
        duration(config.session_absolute)?;
        duration(config.session_idle)?;
        *self
            .inner
            .policy
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = config;
        Ok(())
    }

    #[must_use]
    pub fn secure_cookie(&self) -> bool {
        self.policy().secure_cookie
    }

    #[must_use]
    pub fn session_absolute_seconds(&self) -> u64 {
        self.policy().session_absolute.as_secs()
    }

    #[must_use]
    pub fn session_idle_seconds(&self) -> u64 {
        self.policy().session_idle.as_secs()
    }

    /// Reports whether the singleton administrator credential exists and is valid.
    ///
    /// # Errors
    /// Returns an error when credential persistence or decoding fails.
    pub async fn initialized(&self) -> Result<bool, AuthError> {
        self.inner
            .password
            .load(&self.inner.store)
            .await
            .map(|credential| credential.is_some())
    }

    /// Verifies that authentication has completed first-admin setup.
    ///
    /// # Errors
    /// Returns an error when no valid administrator credential is available.
    pub async fn check_ready(&self) -> Result<(), AuthError> {
        if self.initialized().await? {
            Ok(())
        } else {
            Err(AuthError::Unauthorized)
        }
    }

    /// Stores the first administrator password and creates its initial session.
    ///
    /// # Errors
    /// Returns a conflict when setup already completed, or a hashing/persistence error.
    pub async fn setup(
        &self,
        password: SecretString,
        now: DateTime<Utc>,
    ) -> Result<IssuedSession, AuthError> {
        if self.initialized().await? {
            return Err(AuthError::Conflict);
        }
        let loaded = self.inner.password.hash(password).await?;
        if !self
            .inner
            .store
            .insert_administrator_credential(loaded.encoded(), now)
            .await?
        {
            return Err(AuthError::Conflict);
        }
        self.inner.password.cache(Arc::clone(&loaded)).await;
        self.issue_session(loaded.fingerprint(), now).await
    }

    /// Verifies a login attempt and creates a digest-only durable session.
    ///
    /// # Errors
    /// Returns a typed authentication, throttling, credential, or persistence error.
    pub async fn login(
        &self,
        address: IpAddr,
        password: &SecretString,
        now: DateTime<Utc>,
    ) -> Result<IssuedSession, AuthError> {
        let Some(loaded) = self.inner.password.load(&self.inner.store).await? else {
            return Err(AuthError::Unauthorized);
        };
        let password_matches = self
            .inner
            .password
            .verify(Arc::clone(&loaded), password.clone())
            .await?;
        if !password_matches {
            return if self.inner.limiter.record_failure(address, now) {
                Err(AuthError::RateLimited)
            } else {
                Err(AuthError::Unauthorized)
            };
        }
        self.inner.limiter.clear(address);
        self.issue_session(loaded.fingerprint(), now).await
    }

    pub(super) fn policy(&self) -> AuthConfig {
        self.inner
            .policy
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

pub(super) fn duration(value: std::time::Duration) -> Result<Duration, AuthError> {
    Duration::from_std(value).map_err(|_| AuthError::InvalidLifetime)
}
