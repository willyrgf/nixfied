mod common;

use std::process::Command;

use common::{TempDir, runtime_binary};

#[test]
fn existing_commands_print_help_without_touching_the_model_or_state() {
    let temp = TempDir::new();
    let missing_model = temp.path.join("missing-model.json");
    let state_base = temp.path.join("state");
    for (command, expected) in [
        ("check", include_str!("fixtures/help/check.txt")),
        ("run", include_str!("fixtures/help/run.txt")),
        ("ps", include_str!("fixtures/help/ps.txt")),
        ("down", include_str!("fixtures/help/down.txt")),
        ("clean", include_str!("fixtures/help/clean.txt")),
    ] {
        let mut scenarios = vec![vec![]];
        if command == "run" {
            scenarios.push(vec!["--task", "smoke"]);
        }
        for args in scenarios {
            for help in ["-h", "--help"] {
                let output = Command::new(runtime_binary())
                    .arg(command)
                    .arg("--model")
                    .arg(&missing_model)
                    .args(&args)
                    .arg(help)
                    .env("NIXFIED_STATE_DIR", &state_base)
                    .output()
                    .expect("runtime help should run");
                assert!(
                    output.status.success(),
                    "{command} {args:?} {help} failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    output.stderr.is_empty(),
                    "{command} {args:?} {help} wrote stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    output.stdout,
                    expected.as_bytes(),
                    "{command} {args:?} {help}"
                );
            }
        }
        for help in ["-h", "--help"] {
            let output = Command::new(runtime_binary())
                .args([command, "--unknown", "--model", help])
                .output()
                .expect("help should precede option validation");
            assert!(output.status.success(), "{command} {help} precedence");
            assert_eq!(
                output.stdout,
                expected.as_bytes(),
                "{command} {help} precedence"
            );
            assert!(output.stderr.is_empty(), "{command} {help} precedence");
        }
    }
    assert!(!missing_model.exists(), "help must not materialise a model");
    assert!(!state_base.exists(), "help must not materialise state");
}

#[test]
fn entry_encoding_precedes_admission() {
    let no_args = Command::new(runtime_binary()).output().unwrap();
    assert!(!no_args.status.success());
    assert!(String::from_utf8_lossy(&no_args.stderr).contains("missing --model path"));
    let unknown = Command::new(runtime_binary())
        .args(["unknown", "--help"])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert!(unknown.stdout.is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let output = Command::new(runtime_binary())
            .arg("run")
            .arg(std::ffi::OsString::from_vec(vec![0xff]))
            .arg("--help")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty()); // Panic text is deliberately not an oracle.
    }
}
