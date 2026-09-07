use std::error::Error;

use argon2::password_hash::PasswordHash;
use tempfile::TempDir;
use videnoa_controller::auth::{hash_password, AuthError, AuthService};
use videnoa_controller::config::ControllerConfig;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};

#[tokio::test]
async fn production_password_hash_preserves_argon2id_parameters_and_verification(
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let hash = hash_password("test-only-production-password")?;
    let parsed = PasswordHash::new(&hash).map_err(|error| error.to_string())?;
    assert_eq!(parsed.algorithm.as_str(), "argon2id");
    assert_eq!(parsed.version, Some(19));
    assert_eq!(parsed.params.get_decimal("m"), Some(19_456));
    assert_eq!(parsed.params.get_decimal("t"), Some(2));
    assert_eq!(parsed.params.get_decimal("p"), Some(1));
    assert!(parsed.salt.is_some());
    assert_eq!(parsed.hash.ok_or("hash output missing")?.len(), 32);

    let directory = TempDir::new()?;
    let database =
        Database::open(DatabaseOptions::new(directory.path().join("auth.sqlite3"))).await?;
    let store = Store::new(database);
    store
        .insert_administrator_credential(&hash, chrono::Utc::now())
        .await?;
    let auth = AuthService::new(ControllerConfig::default().auth, store)?;
    let peer = std::net::IpAddr::from([127, 0, 0, 1]);
    auth.authenticate_bearer(peer, "test-only-production-password", chrono::Utc::now())
        .await?;
    assert!(matches!(
        auth.authenticate_bearer(peer, "test-only-wrong-password", chrono::Utc::now())
            .await,
        Err(AuthError::Unauthorized)
    ));
    Ok(())
}
