use std::error::Error;
use std::process::Command;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[test]
fn legacy_hash_password_subcommand_is_not_exposed() -> TestResult {
    // Given: a caller attempts to use the removed manual PHC generation command.
    let mut command = Command::new(env!("CARGO_BIN_EXE_videnoa-controller"));
    command.arg("hash-password");

    // When: the Controller parses its command line.
    let output = command.output()?;

    // Then: Clap rejects the legacy command and emits no credential material.
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn docker_listener_overrides_are_typed_and_accepted() -> TestResult {
    // Given: the listener overrides used by the controller container entrypoint.
    let mut command = Command::new(env!("CARGO_BIN_EXE_videnoa-controller"));
    command.args(["--host", "0.0.0.0", "--port", "3001", "--help"]);

    // When: Clap parses the complete invocation without starting the service.
    let output = command.output()?;

    // Then: the typed host and nonzero port overrides are accepted.
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    Ok(())
}
