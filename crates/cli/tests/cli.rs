//! Runs the real `leadgen` binary like a user would.
//! Every test here fails before Chrome starts, so none of them touch Google.

use std::io::Write;

use assert_cmd::Command;
use predicates::str::contains;

fn leadgen() -> Command {
    Command::cargo_bin("leadgen").unwrap()
}

#[test]
fn help_shows_usage() {
    leadgen()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Usage: leadgen"));
}

#[test]
fn version_matches_cargo_toml() {
    leadgen()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn no_queries_is_a_usage_error() {
    leadgen()
        .assert()
        .code(2)
        .stderr(contains("required arguments were not provided"));
}

#[test]
fn zero_concurrency_is_a_usage_error() {
    leadgen()
        .args(["plumber", "-c", "0"])
        .assert()
        .code(2)
        .stderr(contains("--concurrency"));
}

#[test]
fn missing_query_file_has_clear_error() {
    leadgen()
        .args(["-f", "does_not_exist.txt"])
        .assert()
        .code(1)
        .stderr(contains("failed to read does_not_exist.txt"));
}

#[test]
fn query_file_with_only_comments_has_clear_error() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "# just a comment\n\n   ").unwrap();

    leadgen()
        .arg("-f")
        .arg(file.path())
        .assert()
        .code(1)
        .stderr(contains("no queries given"));
}
