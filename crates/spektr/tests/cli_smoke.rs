//! Smoke test: the binary builds, runs, and reports its version.

use assert_cmd::Command;

#[test]
fn version_flag_prints_package_version() {
    let out = Command::cargo_bin("spektr")
        .unwrap()
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout.trim(),
        format!("spektr {}", env!("CARGO_PKG_VERSION"))
    );
}
