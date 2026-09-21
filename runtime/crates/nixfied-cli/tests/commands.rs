use std::process::Command;

#[test]
fn exact_usage_and_native_help_precedence() {
    let usage = include_str!("fixtures/install-help.txt");
    let root_usage = format!("nixfied\n{usage}");
    for args in [vec![], vec!["help"], vec!["-h"], vec!["--help"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_nixfied"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, root_usage.as_bytes());
        assert!(output.stderr.is_empty());
    }
    for help in ["-h", "--help"] {
        let output = Command::new(env!("CARGO_BIN_EXE_nixfied"))
            .args(["install", "--unknown", "--root", help])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, usage.as_bytes());
        assert!(output.stderr.is_empty());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_nixfied"))
        .args(["unknown", "--help"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"unsupported nixfied command: unknown\n");
}

#[cfg(unix)]
#[test]
fn root_invalid_encoding_is_usage_but_install_help_wins_over_invalid_encoding() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let invalid = || OsString::from_vec(vec![0xff]);
    let output = Command::new(env!("CARGO_BIN_EXE_nixfied"))
        .arg(invalid())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        format!("nixfied\n{}", include_str!("fixtures/install-help.txt")).as_bytes()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_nixfied"))
        .arg("install")
        .arg(invalid())
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, include_bytes!("fixtures/install-help.txt"));
}
