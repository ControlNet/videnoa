use std::error::Error;
use std::io::Write;
use std::process::{Command, Stdio};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[test]
fn hash_password_writes_only_an_argon2id_phc_string() -> TestResult {
    // Given: the Controller CLI receives a password through its protected input stream.
    let mut child = Command::new(env!("CARGO_BIN_EXE_videnoa-controller"))
        .arg("hash-password")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("hash command stdin is unavailable"))?
        .write_all(b"cli-test-password\n")?;

    // When: the command completes.
    let output = child.wait_with_output()?;
    let stdout = String::from_utf8(output.stdout)?;

    // Then: stdout is a usable PHC value and never contains the raw password.
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8(output.stderr)?
    );
    assert!(stdout.trim().starts_with("$argon2id$v=19$"));
    assert!(!stdout.contains("cli-test-password"));
    Ok(())
}
