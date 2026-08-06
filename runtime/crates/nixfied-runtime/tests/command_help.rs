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
