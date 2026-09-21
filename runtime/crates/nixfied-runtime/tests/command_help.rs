mod common;

use std::process::Command;

use common::{TempDir, runtime_binary};

struct HelpCase {
    command: &'static str,
    injected_args: &'static [&'static str],
    usage: &'static str,
    public_flags: &'static [&'static str],
}

#[test]
fn existing_commands_print_help_without_touching_the_model_or_state() {
    let temp = TempDir::new();
    let missing_model = temp.path.join("missing-model.json");
    let state_base = temp.path.join("state");
    let cases = [
        HelpCase {
            command: "check",
            injected_args: &[],
            usage: "nix run .#model-check",
            public_flags: &["--slot"],
        },
        HelpCase {
            command: "run",
            injected_args: &[],
            usage: "nix run .#run",
            public_flags: &[
                "--task",
                "--slot",
                "--timeout-ms",
                "--output",
                "summary",
                "json",
                "both",
                "task-output",
            ],
        },
        HelpCase {
            command: "run",
            injected_args: &["--task", "smoke"],
            usage: "nix run .#<verb>",
            public_flags: &["--slot", "--timeout-ms", "--output", "summary", "json"],
        },
        HelpCase {
            command: "ps",
            injected_args: &[],
            usage: "nix run .#ps",
            public_flags: &["--slot"],
        },
        HelpCase {
            command: "down",
            injected_args: &[],
            usage: "nix run .#down",
            public_flags: &["--slot", "--timeout-ms"],
        },
        HelpCase {
            command: "clean",
            injected_args: &[],
            usage: "nix run .#clean",
            public_flags: &["--slot", "--purge"],
        },
    ];

    for case in cases {
        for help_flag in ["-h", "--help"] {
            let output = Command::new(runtime_binary())
                .arg(case.command)
                .arg("--model")
                .arg(&missing_model)
                .args(case.injected_args)
                .arg(help_flag)
                .env("NIXFIED_STATE_DIR", &state_base)
                .output()
                .expect("runtime help should run");
            assert!(
                output.status.success(),
                "{} {help_flag} failed: {}",
                case.command,
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                output.stderr.is_empty(),
                "{} {help_flag} wrote stderr: {}",
                case.command,
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");
            assert!(
                stdout.contains(case.usage),
                "{} {help_flag} omitted usage {usage:?}: {stdout}",
                case.command,
                usage = case.usage
            );
            assert!(stdout.contains("-h, --help"));
            assert!(!stdout.contains("--model"));
            assert!(!stdout.contains("--allow-non-store-model"));
            assert!(!stdout.contains("--state-base"));
            for public_flag in case.public_flags {
                assert!(
                    stdout.contains(public_flag),
                    "{} {help_flag} omitted {public_flag}: {stdout}",
                    case.command
                );
            }
            if case.command == "run" {
                assert!(!stdout.contains("--task-output"));
                assert!(!stdout.contains("task_output"));
                assert!(!stdout.contains("taskOutput"));
            }
        }
    }

    assert!(!missing_model.exists(), "help must not materialise a model");
    assert!(!state_base.exists(), "help must not materialise state");
}

#[test]
fn exact_help_and_entry_encoding_precede_admission() {
    for (command, expected) in [
        ("check", include_str!("fixtures/help/check.txt")),
        ("run", include_str!("fixtures/help/run.txt")),
        ("ps", include_str!("fixtures/help/ps.txt")),
        ("down", include_str!("fixtures/help/down.txt")),
        ("clean", include_str!("fixtures/help/clean.txt")),
    ] {
        for help in ["-h", "--help"] {
            let output = Command::new(runtime_binary())
                .args([command, "--unknown", "--model", help])
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, expected.as_bytes());
            assert!(output.stderr.is_empty());
        }
    }
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
