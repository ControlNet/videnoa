use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use rand_core::{OsRng, RngCore};
use subtle::ConstantTimeEq;

use crate::config::AuthConfig;
use crate::domain::{AuthMethod, LoginResponse, SecretString, SessionId, SessionResponse};
use crate::persistence::{NewSession, PersistenceError, SessionRecord, Store};

use super::limiter::LoginLimiter;
use super::password::{digest, PasswordFile};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("authentication failed")]
    Unauthorized,
    #[error("request proof is invalid")]
    Forbidden,
    #[error("too many login attempts")]
    RateLimited,
    #[error("password hash is not a valid Argon2id PHC string")]
    InvalidPasswordHash,
    #[error("password hashing failed")]
    PasswordHashing,
    #[error("failed to read password hash file: {path}")]
    PasswordFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("session lifetime cannot be represented")]
    InvalidLifetime,
    #[error("session persistence failed")]
    Persistence(#[from] PersistenceError),
}

#[derive(Clone)]
pub struct AuthService {
    inner: Arc<AuthServiceInner>,
}

struct AuthServiceInner {
    config: AuthConfig,
    password: PasswordFile,
    store: Store,
    limiter: LoginLimiter,
}

pub struct IssuedSession {
    pub response: LoginResponse,
    pub token: String,
    pub csrf: String,
}

impl AuthService {
    /// Opens the configured credential boundary and durable session store.
    ///
    /// # Errors
    /// Returns an error when the password file is unreadable or invalid.
    pub fn new(config: AuthConfig, store: Store) -> Result<Self, AuthError> {
        let password = PasswordFile::new(&config.password_hash_file)?;
        Ok(Self {
            inner: Arc::new(AuthServiceInner {
                config,
                password,
                store,
                limiter: LoginLimiter::default(),
            }),
        })
    }

    #[must_use]
    pub fn secure_cookie(&self) -> bool {
        self.inner.config.secure_cookie
    }

    #[must_use]
    pub fn session_absolute_seconds(&self) -> u64 {
        self.inner.config.session_absolute.as_secs()
    }

    #[must_use]
    pub fn session_idle_seconds(&self) -> u64 {
        self.inner.config.session_idle.as_secs()
    }

    /// # Errors
    /// Returns an authentication error when the credential file cannot be loaded.
    pub fn check_ready(&self) -> Result<(), AuthError> {
        self.inner.password.load().map(|_| ())
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
        let loaded = self.inner.password.load()?;
        if !loaded.verify(password) {
            return if self.inner.limiter.record_failure(address, now) {
                Err(AuthError::RateLimited)
            } else {
                Err(AuthError::Unauthorized)
            };
        }
        self.inner.limiter.clear(address);
        let absolute = duration(self.inner.config.session_absolute)?;
        let idle = duration(self.inner.config.session_idle)?;
        let absolute_expires_at = now + absolute;
        let idle_expires_at = now + idle;
        let token = random_secret();
        let csrf = random_secret();
        let id = SessionId::random();
        self.inner
            .store
            .insert_session(&NewSession {
                id,
                token_digest: digest(token.as_bytes()),
                csrf_digest: digest(csrf.as_bytes()),
                password_hash_fingerprint: loaded.fingerprint(),
                absolute_expires_at,
                idle_expires_at,
                created_at: now,
            })
            .await?;
        Ok(IssuedSession {
            response: LoginResponse {
                session: session_response(
                    id,
                    AuthMethod::Session,
                    absolute_expires_at,
                    idle_expires_at,
                ),
            },
            token,
            csrf,
        })
    }

    /// Validates and touches a durable cookie session at the supplied instant.
    ///
    /// # Errors
    /// Returns [`AuthError::Unauthorized`] for absent, expired, revoked, or rotated sessions.
    pub async fn authenticate_session_at(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> Result<SessionRecord, AuthError> {
        let mut session = self.valid_session(token, now).await?;
        let idle = duration(self.inner.config.session_idle)?;
        session.last_used_at = now;
        session.idle_expires_at = std::cmp::min(now + idle, session.absolute_expires_at);
        if !self
            .inner
            .store
            .touch_session(session.id, now, session.idle_expires_at)
            .await?
        {
            return Err(AuthError::Unauthorized);
        }
        Ok(session)
    }

    /// Validates a durable cookie session without extending its idle lifetime.
    ///
    /// # Errors
    /// Returns [`AuthError::Unauthorized`] for absent, expired, revoked, or rotated sessions.
    pub async fn validate_session_at(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> Result<SessionRecord, AuthError> {
        self.valid_session(token, now).await
    }

    async fn valid_session(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> Result<SessionRecord, AuthError> {
        let Some(session) = self
            .inner
            .store
            .session_by_token_digest(digest(token.as_bytes()))
            .await?
        else {
            return Err(AuthError::Unauthorized);
        };
        let current = self.inner.password.load()?;
        let fingerprint_matches = bool::from(
            session
                .password_hash_fingerprint
                .as_bytes()
                .ct_eq(current.fingerprint().as_bytes()),
        );
        if session.revoked_at.is_some()
            || session.absolute_expires_at <= now
            || session.idle_expires_at <= now
            || !fingerprint_matches
        {
            let _ = self.inner.store.revoke_session(session.id, now).await?;
            return Err(AuthError::Unauthorized);
        }
        Ok(session)
    }

    /// Verifies a raw bearer password against the current hash file.
    ///
    /// # Errors
    /// Returns [`AuthError::Unauthorized`] when the password does not match.
    pub fn authenticate_bearer(&self, password: &str) -> Result<(), AuthError> {
        if self
            .inner
            .password
            .load()?
            .verify(&SecretString::new(password))
        {
            Ok(())
        } else {
            Err(AuthError::Unauthorized)
        }
    }

    /// Replaces a session's CSRF digest and returns the new raw proof.
    ///
    /// # Errors
    /// Returns an error when the session is unavailable or persistence fails.
    pub async fn rotate_csrf(&self, session: SessionId) -> Result<String, AuthError> {
        let csrf = random_secret();
        if self
            .inner
            .store
            .update_session_csrf(session, digest(csrf.as_bytes()))
            .await?
        {
            Ok(csrf)
        } else {
            Err(AuthError::Unauthorized)
        }
    }

    /// Revokes a durable session.
    ///
    /// # Errors
    /// Returns an error when persistence fails.
    pub async fn logout(&self, session: SessionId, now: DateTime<Utc>) -> Result<(), AuthError> {
        self.inner.store.revoke_session(session, now).await?;
        Ok(())
    }

    #[must_use]
    pub fn csrf_matches(session: &SessionRecord, csrf: &str) -> bool {
        bool::from(
            session
                .csrf_digest
                .as_bytes()
                .ct_eq(digest(csrf.as_bytes()).as_bytes()),
        )
    }
}

fn duration(value: std::time::Duration) -> Result<Duration, AuthError> {
    Duration::from_std(value).map_err(|_| AuthError::InvalidLifetime)
}

fn random_secret() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn session_response(
    id: SessionId,
    method: AuthMethod,
    expires_at: DateTime<Utc>,
    idle_expires_at: DateTime<Utc>,
) -> SessionResponse {
    SessionResponse {
        id,
        authenticated: true,
        method,
        expires_at,
        idle_expires_at,
    }
}
