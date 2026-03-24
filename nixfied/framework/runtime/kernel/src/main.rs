use std::env;

fn usage(program: &str) {
    println!("Usage: {} <command> [arguments]", program);
    println!("Commands:");
    println!("  validate-input <payload>");
    println!("  validate-scalar <scalar>");
    println!("  validate-payload <payload>");
    println!("  validate-artifact <artifact>");
    println!("  machine-output run <payload-file> <target>");
    println!("  run-record create <path> <run-id>");
    println!("  run-record transition <path> <run-id> <state>");
    println!("  registry append <path>");
    println!("  registry replay <path>");
    println!("  summary write <run-id>");
    println!("  summary render-human <run-id>");
    println!("  probe evaluate <plan>");
    println!("  adapter decode <kind> <payload>");
}

fn stub(cmd: &str, args: &[String], required: usize) -> Result<(), i32> {
    if args.len() < required {
        eprintln!("ERROR: {} requires at least {} arguments", cmd, required);
        return Err(2);
    }

    println!("OK: {} delegated to kernel", cmd);
    for arg in args {
        println!("ARG: {}", arg);
    }
    Ok(())
}

fn main() {
    let mut args = env::args().collect::<Vec<_>>();

    if args.is_empty() {
        return;
    }

    let program = args.remove(0);
    if args.is_empty() {
        usage(&program);
        std::process::exit(1);
    }

    let cmd = args.remove(0);
    let result = match cmd.as_str() {
        "validate-input" => stub("validate-input", &args, 1),
        "validate-scalar" => stub("validate-scalar", &args, 1),
        "validate-payload" => stub("validate-payload", &args, 1),
        "validate-artifact" => stub("validate-artifact", &args, 1),
        "machine-output" => {
            if args.is_empty() {
                eprintln!("ERROR: machine-output requires a subcommand");
                Err(2)
            } else {
                let sub = args.remove(0);
                if sub != "run" {
                    eprintln!("ERROR: unknown machine-output subcommand: {}", sub);
                    Err(2)
                } else {
                    stub("machine-output run", &args, 2)
                }
            }
        }
        "run-record" => {
            if args.is_empty() {
                eprintln!("ERROR: run-record requires a subcommand");
                Err(2)
            } else {
                let sub = args.remove(0);
                if sub == "create" {
                    stub("run-record create", &args, 2)
                } else if sub == "transition" {
                    stub("run-record transition", &args, 3)
                } else {
                    eprintln!("ERROR: unknown run-record subcommand: {}", sub);
                    Err(2)
                }
            }
        }
        "registry" => {
            if args.is_empty() {
                eprintln!("ERROR: registry requires a subcommand");
                Err(2)
            } else {
                let sub = args.remove(0);
                if sub == "append" {
                    stub("registry append", &args, 1)
                } else if sub == "replay" {
                    stub("registry replay", &args, 1)
                } else {
                    eprintln!("ERROR: unknown registry subcommand: {}", sub);
                    Err(2)
                }
            }
        }
        "summary" => {
            if args.is_empty() {
                eprintln!("ERROR: summary requires a subcommand");
                Err(2)
            } else {
                let sub = args.remove(0);
                if sub == "write" {
                    stub("summary write", &args, 1)
                } else if sub == "render-human" {
                    stub("summary render-human", &args, 1)
                } else {
                    eprintln!("ERROR: unknown summary subcommand: {}", sub);
                    Err(2)
                }
            }
        }
        "probe" => {
            if args.is_empty() {
                eprintln!("ERROR: probe requires a subcommand");
                Err(2)
            } else {
                let sub = args.remove(0);
                if sub == "evaluate" {
                    stub("probe evaluate", &args, 1)
                } else {
                    eprintln!("ERROR: unknown probe subcommand: {}", sub);
                    Err(2)
                }
            }
        }
        "adapter" => {
            if args.is_empty() {
                eprintln!("ERROR: adapter requires a subcommand");
                Err(2)
            } else {
                let sub = args.remove(0);
                if sub == "decode" {
                    stub("adapter decode", &args, 2)
                } else {
                    eprintln!("ERROR: unknown adapter subcommand: {}", sub);
                    Err(2)
                }
            }
        }
        "--help" | "-h" => {
            usage(&program);
            Ok(())
        }
        _ => {
            usage(&program);
            Err(2)
        }
    };

    if let Err(code) = result {
        std::process::exit(code);
    }
}

