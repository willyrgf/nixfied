use super::*;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn native_installer_defaults_operands_and_repetition() {
    let initial = InstallOptions::parse(&[]).unwrap();
    assert_eq!(initial.root, PathBuf::from("."));
    assert_eq!(initial.project_id, None);
    assert_eq!(initial.name, None);
    assert_eq!(initial.nixfied_url, "github:willyrgf/nixfied");
    let options = InstallOptions::parse(&args(&[
        "--root",
        "first",
        "--root",
        "--literal",
        "--name",
        "first",
        "--name",
        "",
        "--project-id",
        "a",
        "--project-id",
        "--b",
        "--nixfied-url",
        "a",
        "--nixfied-url",
        "",
    ]))
    .unwrap();
    assert_eq!(options.root, PathBuf::from("--literal"));
    assert_eq!(options.name.as_deref(), Some(""));
    assert_eq!(options.project_id.as_deref(), Some("--b"));
    assert_eq!(options.nixfied_url, "");
    for flag in ["--root", "--project-id", "--name", "--nixfied-url"] {
        let error = InstallOptions::parse(&args(&[flag])).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert_eq!(error.message, format!("missing {flag} value"));
    }
    for token in ["--", "--root=.", "position", "-hh"] {
        assert_eq!(
            InstallOptions::parse(&args(&[token])).unwrap_err().message,
            format!("unknown install argument: {token}")
        );
    }
}

#[cfg(unix)]
#[test]
fn native_installer_encoding_errors_remain_branch_specific() {
    use std::os::unix::ffi::OsStringExt;
    let invalid = || OsString::from_vec(vec![0xff]);
    assert_eq!(
        InstallOptions::parse(&[invalid()]).unwrap_err().message,
        "arguments must be valid UTF-8"
    );
    for flag in ["--root", "--project-id", "--name", "--nixfied-url"] {
        assert_eq!(
            InstallOptions::parse(&[flag.into(), invalid()])
                .unwrap_err()
                .message,
            format!("missing {flag} value")
        );
    }
}
