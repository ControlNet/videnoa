use std::net::IpAddr;
use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use rand_core::{OsRng, RngCore};
use subtle::ConstantTimeEq;

use crate::domain::{AuthMethod, LoginResponse, SecretString, SessionId, SessionResponse};
use crate::persistence::{AuthDigest, NewSession, SessionRecord};

use super::authentication_service::{duration, IssuedSession};
use super::credentials::digest;
use super::{AuthError, AuthService};

impl AuthService {
    pub(super) async fn issue_session(
        &self,
        password_hash_fingerprint: AuthDigest,
        now: DateTime<Utc>,
    ) -> Result<IssuedSession, AuthError> {
        let policy = self.policy();
        let absolute_expires_at = now + duration(policy.session_absolute)?;
        let idle_expires_at = now + duration(policy.session_idle)?;
        let password_hash_fingerprint =
            policy_fingerprint(password_hash_fingerprint, policy.secure_cookie);
        let token = random_secret();
        let csrf = random_secret();
        let id = SessionId::random();
        self.inner
            .store
            .insert_session(&NewSession {
                id,
                token_digest: digest(token.as_bytes()),
                csrf_digest: digest(csrf.as_bytes()),
                password_hash_fingerprint,
                absolute_expires_at,
                idle_expires_at,
                created_at: now,
            })
            .await?;
        Ok(IssuedSession {
            response: LoginResponse {
                session: response(
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
        let idle = duration(self.policy().session_idle)?;
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
        let Some(mut session) = self
            .inner
            .store
            .session_by_token_digest(digest(token.as_bytes()))
            .await?
        else {
            return Err(AuthError::Unauthorized);
        };
        let Some(current) = self.inner.password.load(&self.inner.store).await? else {
            return Err(AuthError::Unauthorized);
        };
        let policy = self.policy();
        session.absolute_expires_at = std::cmp::min(
            session.absolute_expires_at,
            session.created_at + duration(policy.session_absolute)?,
        );
        let current_fingerprint = policy_fingerprint(current.fingerprint(), policy.secure_cookie);
        let fingerprint_matches = bool::from(
            session
                .password_hash_fingerprint
                .as_bytes()
                .ct_eq(current_fingerprint.as_bytes()),
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

    /// Verifies a raw bearer password against the current administrator credential.
    ///
    /// # Errors
    /// Returns a typed authentication or throttling error when the password does not match.
    pub async fn authenticate_bearer(
        &self,
        address: IpAddr,
        password: &str,
        now: DateTime<Utc>,
    ) -> Result<(), AuthError> {
        let Some(loaded) = self.inner.password.load(&self.inner.store).await? else {
            return Err(AuthError::Unauthorized);
        };
        let password_matches = self
            .inner
            .password
            .verify(Arc::clone(&loaded), SecretString::new(password))
            .await?;
        if password_matches {
            self.inner.limiter.clear(address);
            Ok(())
        } else if self.inner.limiter.record_failure(address, now) {
            Err(AuthError::RateLimited)
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

fn random_secret() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn policy_fingerprint(credential: AuthDigest, secure_cookie: bool) -> AuthDigest {
    if secure_cookie {
        let mut policy = [0_u8; 33];
        policy[..32].copy_from_slice(credential.as_bytes());
        policy[32] = 1;
        digest(&policy)
    } else {
        credential
    }
}

pub fn response(
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
