//! Optional service authentication. WebSockets and health checks remain public.
use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use rand_core::OsRng;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

use super::AppState;
use crate::config::AuthConfig;

const COOKIE: &str = "videnoa_service_session";

#[derive(Clone)]
pub struct AuthService(Arc<Inner>);
struct Inner {
    database: Mutex<Database>,
    policy: RwLock<AuthConfig>,
    tasks: Arc<Semaphore>,
    _lock: File,
}
struct Database {
    connection: Connection,
    failures: HashMap<IpAddr, VecDeque<i64>>,
    // Ephemeral bearer verifier, guarded by the credential transaction mutex. Never serialized.
    cached_password_digest: Option<[u8; 32]>,
    #[cfg(test)]
    slow_verifications: usize,
}

#[derive(Debug)]
pub(super) enum AuthError {
    Unauthorized,
    Forbidden,
    Invalid,
    RateLimited,
    Unavailable,
}
impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Authentication operation failed: {self:?}")
    }
}
impl std::error::Error for AuthError {}
impl From<rusqlite::Error> for AuthError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Unavailable
    }
}
impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Authentication required or password incorrect"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Request origin or CSRF token is invalid"),
            Self::Invalid => (StatusCode::BAD_REQUEST, "Passwords must match, contain 1 to 1024 UTF-8 bytes, and contain no control characters"),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "Too many incorrect passwords; try again later"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "Authentication storage unavailable"),
        };
        (
            status,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"error": message})),
        )
            .into_response()
    }
}
#[derive(Clone)]
struct RequestContext {
    headers: HeaderMap,
    peer: Option<IpAddr>,
}
impl RequestContext {
    fn new(headers: HeaderMap, peer: Option<ConnectInfo<SocketAddr>>) -> Self {
        Self {
            headers,
            peer: peer.map(|p| p.0.ip()),
        }
    }
    fn same_origin(&self) -> bool {
        let Some(host) = self.headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
            return false;
        };
        let Some(origin) = self
            .headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
        else {
            return false;
        };
        let Ok(url) = url::Url::parse(origin) else {
            return false;
        };
        matches!(url.scheme(), "http" | "https")
            && url.origin().ascii_serialization() == format!("{}://{host}", url.scheme())
    }
    fn cookie(&self) -> Option<&str> {
        self.headers
            .get(header::COOKIE)?
            .to_str()
            .ok()?
            .split(';')
            .map(str::trim)
            .find_map(|part| part.strip_prefix(COOKIE)?.strip_prefix('='))
    }
}
enum Action {
    Check {
        mutation: bool,
    },
    Session,
    Login {
        password: String,
    },
    SetPassword {
        password: String,
        confirmation: String,
    },
    Disable,
    Logout,
}
struct Reply {
    body: Value,
    cookie: Option<String>,
}
impl IntoResponse for Reply {
    fn into_response(self) -> Response {
        let mut response = Json(self.body).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
        if let Some(cookie) = self.cookie {
            response
                .headers_mut()
                .insert(header::SET_COOKIE, cookie.parse().unwrap());
        }
        response
    }
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn valid_password(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
}
fn session_cookie(policy: &AuthConfig, token: &str, clear: bool) -> String {
    format!(
        "{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}{}",
        if clear {
            0
        } else {
            policy.session_absolute_seconds
        },
        if policy.secure_cookie { "; Secure" } else { "" }
    )
}
fn open_database(dir: &Path) -> anyhow::Result<(File, Connection)> {
    std::fs::create_dir_all(dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("auth.lock"))?;
    lock.try_lock().map_err(|_| {
        anyhow::anyhow!(
            "Authentication data is in use; stop this instance before resetting its password"
        )
    })?;
    let database_path = dir.join("auth.sqlite3");
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Create privately before SQLite creates its WAL and shared-memory sidecars.
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(&database_path)?;
    }
    let connection = Connection::open(&database_path)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS credential (id INTEGER PRIMARY KEY CHECK(id=1), password_hash TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS sessions (token_digest TEXT PRIMARY KEY, csrf TEXT NOT NULL, fingerprint TEXT NOT NULL,
            created INTEGER NOT NULL, last_seen INTEGER NOT NULL, expires INTEGER NOT NULL, idle_expires INTEGER NOT NULL, secure INTEGER NOT NULL);")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["auth.sqlite3", "auth.sqlite3-wal", "auth.sqlite3-shm"] {
            let path = dir.join(name);
            if path.exists() {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            }
        }
    }
    Ok((lock, connection))
}
/// Removes only authentication state. Fails while an instance owns this data directory.
pub fn reset(dir: &Path) -> anyhow::Result<()> {
    let (_lock, mut connection) = open_database(dir)?;
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM credential", [])?;
    transaction.execute("DELETE FROM sessions", [])?;
    transaction.commit()?;
    Ok(())
}
impl AuthService {
    pub fn open(dir: &Path, policy: AuthConfig) -> anyhow::Result<Self> {
        policy.validate()?;
        let (lock, connection) = open_database(dir)?;
        if let Some(hash) = stored_hash(&connection)? {
            let parsed = PasswordHash::new(&hash)
                .map_err(|_| anyhow::anyhow!("Invalid stored authentication credential"))?;
            anyhow::ensure!(
                parsed.algorithm.as_str() == "argon2id"
                    && parsed.salt.is_some()
                    && parsed.hash.is_some(),
                "Invalid stored authentication credential"
            );
        }
        Ok(Self(Arc::new(Inner {
            database: Mutex::new(Database {
                connection,
                failures: HashMap::new(),
                cached_password_digest: None,
                #[cfg(test)]
                slow_verifications: 0,
            }),
            policy: RwLock::new(policy),
            tasks: Arc::new(Semaphore::new(2)),
            _lock: lock,
        })))
    }
    pub(super) fn reconfigure(&self, policy: AuthConfig) -> anyhow::Result<()> {
        policy.validate()?;
        *self
            .0
            .policy
            .write()
            .map_err(|_| anyhow::anyhow!("Authentication policy unavailable"))? = policy;
        Ok(())
    }
    async fn execute(&self, context: RequestContext, action: Action) -> Result<Reply, AuthError> {
        let permit = self
            .0
            .tasks
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| AuthError::Unavailable)?;
        let inner = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut database = inner.database.lock().map_err(|_| AuthError::Unavailable)?;
            let policy = inner
                .policy
                .read()
                .map_err(|_| AuthError::Unavailable)?
                .clone();
            database.perform(&context, action, &policy)
        })
        .await
        .map_err(|_| AuthError::Unavailable)?
    }
}
fn stored_hash(conn: &Connection) -> Result<Option<String>, AuthError> {
    Ok(conn
        .query_row("SELECT password_hash FROM credential WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()?)
}
#[derive(Default)]
struct Identity {
    csrf: Option<String>,
    expires: Option<i64>,
    idle_expires: Option<i64>,
}
impl Database {
    fn verify(
        &mut self,
        context: &RequestContext,
        password: &str,
        hash: &str,
    ) -> Result<(), AuthError> {
        let peer = context.peer.ok_or(AuthError::Unavailable)?;
        let now = Utc::now().timestamp();
        self.failures.retain(|_, times| {
            times.retain(|time| *time > now - 300);
            !times.is_empty()
        });
        if self
            .failures
            .get(&peer)
            .is_some_and(|times| times.len() >= 5)
        {
            return Err(AuthError::RateLimited);
        }
        #[cfg(test)]
        {
            self.slow_verifications += 1;
        }
        let parsed = PasswordHash::new(hash).map_err(|_| AuthError::Unavailable)?;
        if valid_password(password)
            && Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        {
            self.failures.remove(&peer);
            Ok(())
        } else {
            self.failures.entry(peer).or_default().push_back(now);
            Err(AuthError::Unauthorized)
        }
    }
    fn authenticate(
        &mut self,
        context: &RequestContext,
        hash: &str,
        policy: &AuthConfig,
        mutation: bool,
    ) -> Result<Identity, AuthError> {
        if let Some(value) = context.headers.get(header::AUTHORIZATION) {
            let password = std::str::from_utf8(value.as_bytes())
                .ok()
                .and_then(|s| s.strip_prefix("Bearer "))
                .ok_or(AuthError::Unauthorized)?;
            let incoming: [u8; 32] = Sha256::digest(password.as_bytes()).into();
            let cached_match = self
                .cached_password_digest
                .as_ref()
                .is_some_and(|cached| bool::from(cached.ct_eq(&incoming)));
            if cached_match {
                if let Some(peer) = context.peer {
                    self.failures.remove(&peer);
                }
            } else {
                self.verify(context, password, hash)?;
                self.cached_password_digest = Some(incoming);
            }
            return Ok(Identity::default());
        }
        let token = context.cookie().ok_or(AuthError::Unauthorized)?;
        let token_digest = digest(token);
        type SessionRow = (String, String, i64, i64, i64, i64, bool);
        let row: Option<SessionRow> = self.connection.query_row("SELECT csrf, fingerprint, created, last_seen, expires, idle_expires, secure FROM sessions WHERE token_digest=?", [&token_digest], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
        let (csrf, fingerprint, created, last_seen, expires, idle_expires, secure) =
            row.ok_or(AuthError::Unauthorized)?;
        let now = Utc::now().timestamp();
        let expires = expires.min(created.saturating_add(policy.session_absolute_seconds as i64));
        let idle_expires =
            idle_expires.min(last_seen.saturating_add(policy.session_idle_seconds as i64));
        if fingerprint != digest(hash)
            || secure != policy.secure_cookie
            || now >= expires
            || now >= idle_expires
        {
            self.connection
                .execute("DELETE FROM sessions WHERE token_digest=?", [&token_digest])?;
            return Err(AuthError::Unauthorized);
        }
        if mutation
            && (!context.same_origin()
                || context
                    .headers
                    .get("x-csrf-token")
                    .and_then(|v| v.to_str().ok())
                    != Some(csrf.as_str()))
        {
            return Err(AuthError::Forbidden);
        }
        let idle_expires = expires.min(now.saturating_add(policy.session_idle_seconds as i64));
        self.connection.execute(
            "UPDATE sessions SET last_seen=?, expires=?, idle_expires=? WHERE token_digest=?",
            params![now, expires, idle_expires, token_digest],
        )?;
        Ok(Identity {
            csrf: Some(csrf),
            expires: Some(expires),
            idle_expires: Some(idle_expires),
        })
    }
    fn issue(&mut self, hash: &str, policy: &AuthConfig) -> Result<Reply, AuthError> {
        let now = Utc::now().timestamp();
        let expires = now
            .checked_add(policy.session_absolute_seconds as i64)
            .ok_or(AuthError::Unavailable)?;
        let idle_expires = now
            .checked_add(policy.session_idle_seconds as i64)
            .ok_or(AuthError::Unavailable)?;
        let token = random_token();
        let csrf = random_token();
        self.connection.execute(
            "DELETE FROM sessions WHERE expires<=? OR idle_expires<=?",
            params![now, now],
        )?;
        self.connection.execute(
            "INSERT INTO sessions VALUES (?,?,?,?,?,?,?,?)",
            params![
                digest(&token),
                csrf,
                digest(hash),
                now,
                now,
                expires,
                idle_expires,
                policy.secure_cookie
            ],
        )?;
        Ok(Reply {
            body: json!({"password_enabled":true,"authenticated":true,"csrf_token":csrf,"expires_at":expires,"idle_expires_at":idle_expires}),
            cookie: Some(session_cookie(policy, &token, false)),
        })
    }
    fn perform(
        &mut self,
        context: &RequestContext,
        action: Action,
        policy: &AuthConfig,
    ) -> Result<Reply, AuthError> {
        let hash = stored_hash(&self.connection)?;
        match action {
            Action::Check { mutation } => {
                if let Some(hash) = hash {
                    self.authenticate(context, &hash, policy, mutation)?;
                }
                Ok(Reply {
                    body: Value::Null,
                    cookie: None,
                })
            }
            Action::Session => {
                let identity = hash
                    .as_ref()
                    .map(|hash| self.authenticate(context, hash, policy, false));
                let identity = match identity {
                    Some(Ok(identity)) => Some(identity),
                    Some(Err(AuthError::Unauthorized)) | None => None,
                    Some(Err(error)) => return Err(error),
                };
                Ok(Reply {
                    body: json!({"password_enabled":hash.is_some(),"authenticated":identity.is_some(),"csrf_token":identity.as_ref().and_then(|v|v.csrf.as_ref()),"expires_at":identity.as_ref().and_then(|v|v.expires),"idle_expires_at":identity.and_then(|v|v.idle_expires)}),
                    cookie: None,
                })
            }
            Action::Login { password } => {
                if !context.same_origin() {
                    return Err(AuthError::Forbidden);
                }
                let hash = hash.ok_or(AuthError::Unauthorized)?;
                self.verify(context, &password, &hash)?;
                self.issue(&hash, policy)
            }
            Action::SetPassword {
                password,
                confirmation,
            } => {
                if let Some(hash) = &hash {
                    self.authenticate(context, hash, policy, true)?;
                } else if !context.same_origin() {
                    return Err(AuthError::Forbidden);
                }
                if !valid_password(&password) || password != confirmation {
                    return Err(AuthError::Invalid);
                }
                let hash = Argon2::default()
                    .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
                    .map_err(|_| AuthError::Unavailable)?
                    .to_string();
                // Issue and credential rotation commit together, including session revocation.
                self.connection.execute_batch("BEGIN IMMEDIATE")?;
                let result = (|| {
                    self.connection.execute("INSERT INTO credential VALUES (1,?) ON CONFLICT(id) DO UPDATE SET password_hash=excluded.password_hash", [&hash])?;
                    self.connection.execute("DELETE FROM sessions", [])?;
                    self.issue(&hash, policy)
                })();
                match result {
                    Ok(reply) => {
                        self.connection.execute_batch("COMMIT")?;
                        self.cached_password_digest =
                            Some(Sha256::digest(password.as_bytes()).into());
                        self.failures.clear();
                        Ok(reply)
                    }
                    Err(error) => {
                        let _ = self.connection.execute_batch("ROLLBACK");
                        Err(error)
                    }
                }
            }
            Action::Disable => {
                if let Some(hash) = &hash {
                    self.authenticate(context, hash, policy, true)?;
                } else if !context.same_origin() {
                    return Err(AuthError::Forbidden);
                }
                let transaction = self.connection.transaction()?;
                transaction.execute("DELETE FROM credential", [])?;
                transaction.execute("DELETE FROM sessions", [])?;
                transaction.commit()?;
                self.cached_password_digest = None;
                self.failures.clear();
                Ok(Reply {
                    body: json!({"password_enabled":false,"authenticated":false}),
                    cookie: Some(session_cookie(policy, "", true)),
                })
            }
            Action::Logout => {
                if let Some(hash) = hash {
                    self.authenticate(context, &hash, policy, true)?;
                } else if !context.same_origin() {
                    return Err(AuthError::Forbidden);
                }
                if let Some(token) = context.cookie() {
                    self.connection
                        .execute("DELETE FROM sessions WHERE token_digest=?", [digest(token)])?;
                }
                Ok(Reply {
                    body: json!({"logged_out":true}),
                    cookie: Some(session_cookie(policy, "", true)),
                })
            }
        }
    }
}
fn service(state: &AppState) -> Result<&AuthService, AuthError> {
    state
        .inner
        .auth
        .as_ref()
        .map_err(|_| AuthError::Unavailable)
}
pub(super) async fn protect(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let context = RequestContext::new(
        request.headers().clone(),
        request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .copied(),
    );
    let action = Action::Check {
        mutation: !matches!(
            *request.method(),
            axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
        ),
    };
    match service(&state) {
        Ok(auth) => match auth.execute(context, action).await {
            Ok(_) => next.run(request).await,
            Err(error) => error.into_response(),
        },
        Err(error) => error.into_response(),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PasswordInput {
    password: String,
    password_confirmation: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LoginInput {
    password: String,
}
macro_rules! simple_handler {
    ($name:ident, $action:expr) => {
        pub(super) async fn $name(
            State(state): State<AppState>,
            peer: Option<Extension<ConnectInfo<SocketAddr>>>,
            headers: HeaderMap,
        ) -> Response {
            match service(&state) {
                Ok(auth) => match auth
                    .execute(RequestContext::new(headers, peer.map(|v| v.0)), $action)
                    .await
                {
                    Ok(reply) => reply.into_response(),
                    Err(error) => error.into_response(),
                },
                Err(error) => error.into_response(),
            }
        }
    };
}
simple_handler!(session, Action::Session);
simple_handler!(logout, Action::Logout);
simple_handler!(disable, Action::Disable);
pub(super) async fn login(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(input): Json<LoginInput>,
) -> Response {
    match service(&state) {
        Ok(auth) => match auth
            .execute(
                RequestContext::new(headers, peer.map(|v| v.0)),
                Action::Login {
                    password: input.password,
                },
            )
            .await
        {
            Ok(reply) => reply.into_response(),
            Err(error) => error.into_response(),
        },
        Err(error) => error.into_response(),
    }
}
pub(super) async fn set_password(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(input): Json<PasswordInput>,
) -> Response {
    match service(&state) {
        Ok(auth) => match auth
            .execute(
                RequestContext::new(headers, peer.map(|v| v.0)),
                Action::SetPassword {
                    password: input.password,
                    confirmation: input.password_confirmation,
                },
            )
            .await
        {
            Ok(reply) => reply.into_response(),
            Err(error) => error.into_response(),
        },
        Err(error) => error.into_response(),
    }
}
#[cfg(test)]
mod tests;
