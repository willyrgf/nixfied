use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{self, Command, Stdio};

fn main() {
    if let Err(err) = run() {
        eprintln!("ERROR: {}", err);
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(usage)?;

    match command.as_str() {
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "validate-payload" | "validate-artifact" => {
            let bundle_path = args.next().ok_or_else(usage)?;
            let contract_ref = args.next().ok_or_else(usage)?;
            let payload_path = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            validate_file_command(&command, &bundle_path, &contract_ref, &payload_path)
        }
        "validate-input" => {
            let plan_path = args.next().ok_or_else(usage)?;
            let mode = args.next().ok_or_else(usage)?;
            let export_path = args.next().ok_or_else(usage)?;
            let remaining = args.collect::<Vec<_>>();
            validate_input_command(&plan_path, &mode, &export_path, &remaining)
        }
        "validate-scalar" => {
            let spec_path = args.next().ok_or_else(usage)?;
            let value = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            validate_scalar_command(&spec_path, &value)
        }
        "validate-exit" => {
            let plan_path = args.next().ok_or_else(usage)?;
            let exit_code = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            validate_exit_command(&plan_path, &exit_code)
        }
        "run-record" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            run_record_command(&subcommand, &values)
        }
        "registry" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            registry_command(&subcommand, &values)
        }
        "summary" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            summary_command(&subcommand, &values)
        }
        "adapter" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            adapter_command(&subcommand, &values)
        }
        "probe" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            probe_command(&subcommand, &values)
        }
        "machine-output" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            machine_output_command(&subcommand, &values)
        }
        other => Err(format!("unknown command: {}\n{}", other, usage())),
    }
}

fn usage() -> String {
    [
        "usage: nixfied-kernel <command> [args...]",
        "",
        "commands:",
        "  validate-payload <bundle-file> <contract-ref> <payload-file>",
        "  validate-artifact <bundle-file> <contract-ref> <payload-file>",
        "  validate-input <plan-file> <env|args> <export-file> [-- <args...>]",
        "  validate-scalar <spec-file> <value>",
        "  validate-exit <plan-file> <exit-code>",
        "  run-record <create|transition> ...",
        "  registry <append|replay> ...",
        "  summary <write|render-human> ...",
        "  adapter decode <kind> ...",
        "  probe evaluate <plan-file> <payload-file> [export-file]",
        "  machine-output run <plan-file> [-- <args...>]",
        "  help",
    ]
    .join("\n")
}

fn print_help() {
    println!("{}", usage());
}

#[derive(Clone)]
struct ScalarSpec {
    type_name: String,
    values: Vec<String>,
    min: Option<i64>,
    max: Option<i64>,
}

#[derive(Clone)]
struct CommandArgSpec {
    name: String,
    kind: String,
    scalar: ScalarSpec,
    long: String,
    short: String,
    required: bool,
}

#[derive(Clone)]
struct CommandEnvSpec {
    name: String,
    scalar: ScalarSpec,
    required: bool,
    default: Option<String>,
    aliases: Vec<String>,
}

#[derive(Clone)]
struct CommandRuntimePlan {
    allow_unknown_args: bool,
    args: Vec<CommandArgSpec>,
    env: Vec<CommandEnvSpec>,
    failure_codes: BTreeSet<i32>,
}

#[derive(Clone)]
struct ProbePlan {
    probe_kind: String,
    export_var: Option<String>,
}

#[derive(Clone)]
struct MachineOutputPlan {
    app_id: String,
    target_app_id: String,
    contract_ref: String,
    bundle_file: String,
    target_program: String,
    setup_programs: Vec<String>,
    teardown_programs: Vec<String>,
    target_args: Vec<String>,
}

fn validate_input_command(
    plan_path: &str,
    mode: &str,
    export_path: &str,
    remaining: &[String],
) -> Result<(), String> {
    let plan = load_command_runtime_plan(plan_path)?;
    let exports = match mode {
        "env" => validate_input_env(&plan)?,
        "args" => {
            let values = strip_passthrough_separator(remaining);
            validate_input_args(&plan, values)?
        }
        other => {
            return Err(format!(
                "validate-input mode must be env or args (got {})",
                other
            ))
        }
    };

    write_shell_exports(export_path, &exports)?;
    println!("OK: validate-input mode={}", mode);
    Ok(())
}

fn validate_scalar_command(spec_path: &str, value: &str) -> Result<(), String> {
    let spec = load_scalar_spec(spec_path)?;
    validate_scalar_value(&spec, value, "scalar")?;
    println!("OK: validate-scalar");
    Ok(())
}

fn validate_exit_command(plan_path: &str, exit_code_text: &str) -> Result<(), String> {
    let plan = load_command_runtime_plan(plan_path)?;
    let exit_code = parse_i32_text(exit_code_text, "exit code")?;
    if exit_code == 0 || plan.failure_codes.contains(&exit_code) {
        println!("OK: validate-exit");
        Ok(())
    } else {
        Err(format!("undeclared exit code code={}", exit_code))
    }
}

fn load_command_runtime_plan(path: &str) -> Result<CommandRuntimePlan, String> {
    let text = read_text(path)?;
    let value = parse_json(&text)
        .map_err(|err| format!("runtime plan {} is not valid JSON: {}", path, err))?;
    parse_command_runtime_plan(&value)
}

fn parse_command_runtime_plan(value: &JsonValue) -> Result<CommandRuntimePlan, String> {
    value
        .as_object()
        .ok_or_else(|| "runtime plan must be an object".to_string())?;

    let allow_unknown_args = object_bool(value, "allowUnknownArgs").unwrap_or(false);
    let mut args = Vec::new();
    let mut env_specs = Vec::new();
    let mut failure_codes = BTreeSet::new();

    for item in object_array(value, "args").unwrap_or(&[]) {
        args.push(parse_command_arg_spec(item)?);
    }

    for item in object_array(value, "env").unwrap_or(&[]) {
        env_specs.push(parse_command_env_spec(item)?);
    }

    if let Some(codes) = object_field(value, "failureCodes").and_then(JsonValue::as_object) {
        for code in codes.values() {
            let number = json_value_to_i32(code)
                .ok_or_else(|| "runtime plan failureCodes values must be integers".to_string())?;
            failure_codes.insert(number);
        }
    }

    Ok(CommandRuntimePlan {
        allow_unknown_args,
        args,
        env: env_specs,
        failure_codes,
    })
}

fn parse_command_arg_spec(value: &JsonValue) -> Result<CommandArgSpec, String> {
    Ok(CommandArgSpec {
        name: required_string_field(value, "name", "arg spec")?.to_string(),
        kind: required_string_field(value, "kind", "arg spec")?.to_string(),
        scalar: parse_scalar_spec(value)?,
        long: object_string(value, "long").unwrap_or("").to_string(),
        short: object_string(value, "short").unwrap_or("").to_string(),
        required: object_bool(value, "required").unwrap_or(false),
    })
}

fn parse_command_env_spec(value: &JsonValue) -> Result<CommandEnvSpec, String> {
    Ok(CommandEnvSpec {
        name: required_string_field(value, "name", "env spec")?.to_string(),
        scalar: parse_scalar_spec(value)?,
        required: object_bool(value, "required").unwrap_or(false),
        default: object_field(value, "default").and_then(json_value_to_plain_string),
        aliases: array_strings(value, "aliases"),
    })
}

fn load_scalar_spec(path: &str) -> Result<ScalarSpec, String> {
    let text = read_text(path)?;
    let value = parse_json(&text)
        .map_err(|err| format!("scalar spec {} is not valid JSON: {}", path, err))?;
    parse_scalar_spec(&value)
}

fn parse_scalar_spec(value: &JsonValue) -> Result<ScalarSpec, String> {
    Ok(ScalarSpec {
        type_name: object_string(value, "type")
            .unwrap_or("string")
            .to_string(),
        values: array_strings(value, "values"),
        min: object_field(value, "min").and_then(json_value_to_i64),
        max: object_field(value, "max").and_then(json_value_to_i64),
    })
}

fn validate_input_env(plan: &CommandRuntimePlan) -> Result<Vec<(String, String)>, String> {
    let mut exports = Vec::new();

    for spec in &plan.env {
        let value = resolve_env_spec_value(spec)?;
        if value.is_none() && spec.required {
            return Err(format!("required env var missing name={}", spec.name));
        }

        if let Some(value) = value {
            validate_scalar_value(&spec.scalar, &value, &format!("env:{}", spec.name))?;
            exports.push((spec.name.clone(), value.clone()));
            for alias in &spec.aliases {
                exports.push((alias.clone(), value.clone()));
            }
        }
    }

    Ok(exports)
}

fn validate_input_args(
    plan: &CommandRuntimePlan,
    args: &[String],
) -> Result<Vec<(String, String)>, String> {
    let mut exports = Vec::new();
    let mut seen = BTreeSet::new();
    let mut positional_values = Vec::new();
    let mut index = 0usize;
    let mut parse_options = true;

    let spec_by_name = plan
        .args
        .iter()
        .map(|spec| (spec.name.as_str(), spec))
        .collect::<BTreeMap<_, _>>();
    let long_to_name = plan
        .args
        .iter()
        .filter(|spec| !spec.long.is_empty())
        .map(|spec| (spec.long.as_str(), spec.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let short_to_name = plan
        .args
        .iter()
        .filter(|spec| !spec.short.is_empty())
        .map(|spec| (spec.short.as_str(), spec.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let positional_specs = plan
        .args
        .iter()
        .filter(|spec| spec.kind == "positional")
        .collect::<Vec<_>>();

    while index < args.len() {
        let token = &args[index];
        index += 1;

        if !parse_options {
            positional_values.push(token.clone());
            continue;
        }

        if token == "--" {
            parse_options = false;
            continue;
        }

        if let Some((name_token, value)) = token.split_once('=') {
            if name_token.starts_with("--") {
                let Some(name) = long_to_name.get(name_token).copied() else {
                    if plan.allow_unknown_args {
                        continue;
                    }
                    return Err(format!("unknown option token={}", name_token));
                };
                let spec = spec_by_name.get(name).copied().unwrap();
                if spec.kind == "flag" {
                    return Err(format!("flag does not accept a value token={}", name_token));
                }
                validate_scalar_value(&spec.scalar, value, &format!("arg:{}", spec.name))?;
                seen.insert(spec.name.clone());
                exports.push((export_arg_name(&spec.name), value.to_string()));
                continue;
            }
        }

        if token.starts_with("--") {
            let Some(name) = long_to_name.get(token.as_str()).copied() else {
                if plan.allow_unknown_args {
                    continue;
                }
                return Err(format!("unknown option token={}", token));
            };
            let spec = spec_by_name.get(name).copied().unwrap();
            if spec.kind == "flag" {
                seen.insert(spec.name.clone());
                exports.push((export_arg_name(&spec.name), "true".to_string()));
                continue;
            }
            if index >= args.len() {
                return Err(format!("option requires value token={}", token));
            }
            let value = &args[index];
            index += 1;
            validate_scalar_value(&spec.scalar, value, &format!("arg:{}", spec.name))?;
            seen.insert(spec.name.clone());
            exports.push((export_arg_name(&spec.name), value.clone()));
            continue;
        }

        if token.starts_with('-') && token.len() > 1 {
            if let Some(name) = short_to_name.get(token.as_str()).copied() {
                let spec = spec_by_name.get(name).copied().unwrap();
                if spec.kind == "flag" {
                    seen.insert(spec.name.clone());
                    exports.push((export_arg_name(&spec.name), "true".to_string()));
                    continue;
                }
                if index >= args.len() {
                    return Err(format!("option requires value token={}", token));
                }
                let value = &args[index];
                index += 1;
                validate_scalar_value(&spec.scalar, value, &format!("arg:{}", spec.name))?;
                seen.insert(spec.name.clone());
                exports.push((export_arg_name(&spec.name), value.clone()));
                continue;
            }

            if token.len() > 2 {
                let mut cluster_specs = Vec::new();
                let mut cluster_valid = true;
                for short in token[1..].chars() {
                    let short_token = format!("-{}", short);
                    let Some(name) = short_to_name.get(short_token.as_str()).copied() else {
                        cluster_valid = false;
                        break;
                    };
                    let spec = spec_by_name.get(name).copied().unwrap();
                    if spec.kind != "flag" {
                        cluster_valid = false;
                        break;
                    }
                    cluster_specs.push(spec);
                }
                if cluster_valid {
                    for spec in cluster_specs {
                        seen.insert(spec.name.clone());
                        exports.push((export_arg_name(&spec.name), "true".to_string()));
                    }
                    continue;
                }
            }

            if plan.allow_unknown_args {
                continue;
            }
            return Err(format!("unknown option token={}", token));
        }

        positional_values.push(token.clone());
    }

    for (position, spec) in positional_specs.iter().enumerate() {
        if let Some(value) = positional_values.get(position) {
            validate_scalar_value(&spec.scalar, value, &format!("arg:{}", spec.name))?;
            seen.insert(spec.name.clone());
            exports.push((export_arg_name(&spec.name), value.clone()));
        } else if spec.required {
            return Err(format!("missing required positional arg name={}", spec.name));
        }
    }

    if positional_values.len() > positional_specs.len() && !plan.allow_unknown_args {
        return Err(format!(
            "unexpected positional args count={}",
            positional_values.len() - positional_specs.len()
        ));
    }

    for spec in &plan.args {
        if spec.required && !seen.contains(&spec.name) {
            return Err(format!("missing required arg name={}", spec.name));
        }
    }

    Ok(exports)
}

fn resolve_env_spec_value(spec: &CommandEnvSpec) -> Result<Option<String>, String> {
    let canonical = env::var(&spec.name).ok();
    let aliases = spec
        .aliases
        .iter()
        .filter_map(|alias| env::var(alias).ok().map(|value| (alias.clone(), value)))
        .collect::<Vec<_>>();

    if let Some(value) = &canonical {
        if value.is_empty() && is_runtime_primitive_env(&spec.name) {
            return Err(format!(
                "env:{} cannot be empty when set; unset {} to use defaults",
                spec.name, spec.name
            ));
        }
    }

    for (alias, value) in &aliases {
        if value.is_empty() && is_runtime_primitive_env(&spec.name) {
            return Err(format!(
                "env:{} alias={} cannot be empty when set; unset {} to use defaults",
                spec.name, alias, alias
            ));
        }
    }

    let canonical_non_empty = canonical.as_ref().filter(|value| !value.is_empty());
    let alias_non_empty = aliases
        .iter()
        .find(|(_, value)| !value.is_empty())
        .map(|(_, value)| value);

    if let (Some(left), Some(right)) = (canonical_non_empty, alias_non_empty) {
        if left != right {
            return Err(format!(
                "env:{} has conflicting values between {} and alias; set one variable or use matching values",
                spec.name, spec.name
            ));
        }
    }

    let mut first_alias: Option<(&str, &String)> = None;
    for (alias, value) in &aliases {
        if value.is_empty() {
            continue;
        }
        if let Some((previous_alias, previous_value)) = first_alias {
            if previous_value != value {
                return Err(format!(
                    "env:{} has conflicting alias values alias={} and alias={}; set one alias or use matching values",
                    spec.name, previous_alias, alias
                ));
            }
        } else {
            first_alias = Some((alias.as_str(), value));
        }
    }

    Ok(
        canonical_non_empty
            .cloned()
            .or_else(|| first_alias.map(|(_, value)| value.clone()))
            .or_else(|| spec.default.clone()),
    )
}

fn validate_scalar_value(spec: &ScalarSpec, value: &str, label: &str) -> Result<(), String> {
    match spec.type_name.as_str() {
        "string" => {}
        "bool" => match value {
            "1" | "0" | "true" | "false" | "TRUE" | "FALSE" | "yes" | "YES" | "no"
            | "NO" | "on" | "ON" => {}
            _ => return Err(format!("{} must be bool (got '{}')", label, value)),
        },
        "int" => {
            let number = parse_i64_text(value, label)?;
            validate_numeric_range(number, spec, label)?;
        }
        "durationSec" => {
            let number = parse_i64_text(value, label)?;
            if number < 0 {
                return Err(format!("{} must be >= 0 seconds (got '{}')", label, value));
            }
            validate_numeric_range(number, spec, label)?;
        }
        "enum" => {
            if !spec.values.iter().any(|item| item == value) {
                return Err(format!(
                    "{} must be one of {} (got '{}')",
                    label,
                    spec.values.join(","),
                    value
                ));
            }
        }
        "pathAbs" => {
            if !value.starts_with('/') {
                return Err(format!("{} must be an absolute path (got '{}')", label, value));
            }
        }
        "pathRel" => {
            if value.is_empty() || value.starts_with('/') {
                return Err(format!("{} must be a relative path (got '{}')", label, value));
            }
        }
        "port" => {
            let number = parse_i64_text(value, label)?;
            if !(1..=65535).contains(&number) {
                return Err(format!("{} must be port 1-65535 (got '{}')", label, value));
            }
            validate_numeric_range(number, spec, label)?;
        }
        "json" => {
            parse_json(value).map_err(|_| format!("{} must be valid json", label))?;
        }
        other => return Err(format!("unsupported type={} for {}", other, label)),
    }

    Ok(())
}

fn validate_numeric_range(value: i64, spec: &ScalarSpec, label: &str) -> Result<(), String> {
    if let Some(minimum) = spec.min {
        if value < minimum {
            return Err(format!("{} must be >= {} (got '{}')", label, minimum, value));
        }
    }
    if let Some(maximum) = spec.max {
        if value > maximum {
            return Err(format!("{} must be <= {} (got '{}')", label, maximum, value));
        }
    }
    Ok(())
}

fn strip_passthrough_separator(values: &[String]) -> &[String] {
    if values.first().map(|value| value.as_str()) == Some("--") {
        &values[1..]
    } else {
        values
    }
}

fn export_arg_name(name: &str) -> String {
    format!("NIXFIED_ARG_{}", sanitize_name(name))
}

fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '.' | ':' | '/' | '-' => '_',
            other => other.to_ascii_uppercase(),
        })
        .collect()
}

fn is_runtime_primitive_env(name: &str) -> bool {
    matches!(name, "LOG_LEVEL" | "OUTPUT_MODE")
}

fn write_shell_exports(path: &str, values: &[(String, String)]) -> Result<(), String> {
    let mut rendered = String::new();
    for (key, value) in values {
        rendered.push_str("export ");
        rendered.push_str(key);
        rendered.push('=');
        rendered.push_str(&shell_quote(value));
        rendered.push('\n');
    }
    write_text_atomic(path, &rendered)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn write_text_atomic(path: &str, contents: &str) -> Result<(), String> {
    let target = Path::new(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {}", parent.display(), err))?;
    }

    let tmp_path = format!("{}.tmp.{}", path, process::id());
    fs::write(&tmp_path, contents)
        .map_err(|err| format!("failed to write {}: {}", tmp_path, err))?;
    fs::rename(&tmp_path, path)
        .map_err(|err| format!("failed to move {} into {}: {}", tmp_path, path, err))
}

fn required_string_field<'a>(
    value: &'a JsonValue,
    key: &str,
    label: &str,
) -> Result<&'a str, String> {
    object_string(value, key).ok_or_else(|| format!("{} missing string field {}", label, key))
}

fn array_strings(value: &JsonValue, key: &str) -> Vec<String> {
    object_array(value, key)
        .unwrap_or(&[])
        .iter()
        .filter_map(JsonValue::as_string)
        .map(|item| item.to_string())
        .collect()
}

fn json_value_to_plain_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Null => None,
        JsonValue::String(value) => Some(value.clone()),
        JsonValue::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
        JsonValue::Number(number) => Some(number.raw.clone()),
        JsonValue::Array(_) | JsonValue::Object(_) => None,
    }
}

fn json_value_to_i64(value: &JsonValue) -> Option<i64> {
    match value {
        JsonValue::Number(number) if number.integer => number.int_value.and_then(|value| i64::try_from(value).ok()),
        JsonValue::String(value) => value.parse::<i64>().ok(),
        _ => None,
    }
}

fn json_value_to_i32(value: &JsonValue) -> Option<i32> {
    json_value_to_i64(value).and_then(|value| i32::try_from(value).ok())
}

fn parse_i64_text(value: &str, label: &str) -> Result<i64, String> {
    value
        .parse::<i64>()
        .map_err(|_| format!("{} must be int (got '{}')", label, value))
}

fn parse_i32_text(value: &str, label: &str) -> Result<i32, String> {
    value
        .parse::<i32>()
        .map_err(|_| format!("{} must be int (got '{}')", label, value))
}

fn validate_file_command(
    command: &str,
    bundle_path: &str,
    contract_ref: &str,
    payload_path: &str,
) -> Result<(), String> {
    validate_json_file_against_contract(bundle_path, contract_ref, payload_path)?;
    println!("OK: {} contract={}", command, contract_ref);
    Ok(())
}

fn validate_json_file_against_contract(
    bundle_path: &str,
    contract_ref: &str,
    payload_path: &str,
) -> Result<(), String> {
    let bundle_text = read_text(bundle_path)?;
    let payload_text = read_text(payload_path)?;
    let bundle = parse_json(&bundle_text)
        .map_err(|err| format!("bundle {} is not valid JSON: {}", bundle_path, err))?;
    let payload = parse_json(&payload_text)
        .map_err(|err| format!("payload {} is not valid JSON: {}", payload_path, err))?;
    validate_json_value_against_contract(&bundle, contract_ref, &payload)
}

fn validate_json_value_against_contract(
    bundle: &JsonValue,
    contract_ref: &str,
    payload: &JsonValue,
) -> Result<(), String> {
    let context = ValidationContext::new(bundle);
    let schema = context
        .resolve_contract_ref(contract_ref)
        .map_err(|err| format!("contract {} could not be resolved: {}", contract_ref, err))?;

    let mut path = Vec::new();
    let mut ref_stack = Vec::new();
    validate_schema(&context, schema, payload, &mut path, &mut ref_stack)
        .map_err(|err| format!("contract {} failed validation: {}", contract_ref, err))
}

fn run_record_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "create" => run_record_create_command(values),
        "transition" => run_record_transition_command(values),
        other => Err(format!("unknown run-record subcommand: {}", other)),
    }
}

fn run_record_create_command(values: &[String]) -> Result<(), String> {
    if values.len() != 11 {
        return Err(
            "usage: nixfied-kernel run-record create <bundle-file> <run-file> <run-id> <attempt-id> <command> <workflow-id> <task-id> <execution-mode> <process-mode> <ephemeral-enabled> <args-file>"
                .to_string(),
        );
    }

    let bundle_path = &values[0];
    let run_file = &values[1];
    let now = current_utc_timestamp()?;
    let args = parse_json_file(&values[10], "run-record args file")?;

    let history = JsonValue::Array(vec![run_record_history_entry("queued", &now)]);
    let payload = JsonValue::Object(BTreeMap::from([
        ("run_id".to_string(), JsonValue::String(values[2].clone())),
        ("attempt_id".to_string(), JsonValue::String(values[3].clone())),
        ("command".to_string(), JsonValue::String(values[4].clone())),
        ("workflow_id".to_string(), nullable_string_value(&values[5])),
        ("task_id".to_string(), nullable_string_value(&values[6])),
        ("execution_mode".to_string(), JsonValue::String(values[7].clone())),
        ("process_mode".to_string(), JsonValue::String(values[8].clone())),
        (
            "ephemeral_enabled".to_string(),
            JsonValue::Bool(parse_bool_flag(&values[9])?),
        ),
        ("state".to_string(), JsonValue::String("queued".to_string())),
        ("pid".to_string(), JsonValue::Null),
        ("pgid".to_string(), JsonValue::Null),
        ("exit_code".to_string(), JsonValue::Null),
        ("stop_reason".to_string(), JsonValue::Null),
        ("created_at".to_string(), JsonValue::String(now.clone())),
        ("started_at".to_string(), JsonValue::Null),
        ("finished_at".to_string(), JsonValue::Null),
        ("updated_at".to_string(), JsonValue::String(now)),
        ("args".to_string(), args),
        ("history".to_string(), history),
    ]));
    let envelope = JsonValue::Object(BTreeMap::from([
        ("kind".to_string(), JsonValue::String("run-record".to_string())),
        (
            "version".to_string(),
            JsonValue::Number(JsonNumber::from_int(1)),
        ),
        ("payload".to_string(), payload),
    ]));

    validate_and_write_json(bundle_path, "runtime.runRecord", run_file, &envelope)?;
    println!("OK: run-record create");
    Ok(())
}

fn run_record_transition_command(values: &[String]) -> Result<(), String> {
    if values.len() != 7 {
        return Err(
            "usage: nixfied-kernel run-record transition <bundle-file> <run-file> <state> <exit-code|empty> <stop-reason|empty> <pid|empty> <pgid|empty>"
                .to_string(),
        );
    }

    let bundle_path = &values[0];
    let run_file = &values[1];
    let state = &values[2];
    let exit_code = parse_optional_i64(&values[3], "run-record exit_code")?;
    let stop_reason = optional_string_value(&values[4]);
    let pid = parse_optional_i64(&values[5], "run-record pid")?;
    let pgid = parse_optional_i64(&values[6], "run-record pgid")?;
    let now = current_utc_timestamp()?;

    let mut envelope = parse_json_file(run_file, "run-record file")?;
    let payload = object_field_mut(&mut envelope, "payload")
        .ok_or_else(|| "run-record payload is missing".to_string())?;
    let payload_object = payload
        .as_object_mut()
        .ok_or_else(|| "run-record payload must be an object".to_string())?;

    payload_object.insert("state".to_string(), JsonValue::String(state.clone()));
    payload_object.insert("updated_at".to_string(), JsonValue::String(now.clone()));
    if let Some(pid) = pid {
        payload_object.insert("pid".to_string(), JsonValue::Number(JsonNumber::from_int(pid)));
    }
    if let Some(pgid) = pgid {
        payload_object.insert("pgid".to_string(), JsonValue::Number(JsonNumber::from_int(pgid)));
    }
    if payload_object
        .get("started_at")
        .map(|value| matches!(value, JsonValue::Null))
        .unwrap_or(true)
        && state == "running"
    {
        payload_object.insert("started_at".to_string(), JsonValue::String(now.clone()));
    }
    if matches!(state.as_str(), "passed" | "failed" | "canceled") {
        payload_object.insert("finished_at".to_string(), JsonValue::String(now.clone()));
    }
    if let Some(exit_code) = exit_code {
        payload_object.insert(
            "exit_code".to_string(),
            JsonValue::Number(JsonNumber::from_int(exit_code)),
        );
    }
    if let Some(stop_reason) = stop_reason {
        payload_object.insert("stop_reason".to_string(), JsonValue::String(stop_reason));
    }

    let history_value = payload_object
        .get_mut("history")
        .ok_or_else(|| "run-record history is missing".to_string())?;
    let history = history_value
        .as_array_mut()
        .ok_or_else(|| "run-record history must be an array".to_string())?;
    history.push(run_record_history_entry(state, &now));

    validate_and_write_json(bundle_path, "runtime.runRecord", run_file, &envelope)?;
    println!("OK: run-record transition");
    Ok(())
}
fn registry_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "append" => registry_append_command(values),
        "replay" => registry_replay_command(values),
        other => Err(format!("unknown registry subcommand: {}", other)),
    }
}

fn registry_append_command(values: &[String]) -> Result<(), String> {
    if values.len() != 11 {
        return Err(
            "usage: nixfied-kernel registry append <bundle-file> <root> <run-id> <attempt-id> <workflow-id> <task-id> <state> <detail-file> <detail-reason> <detail-exit-code> <export-file>"
                .to_string(),
        );
    }

    let bundle_path = &values[0];
    let root = &values[1];
    let seq_file = format!("{}/.seq", root);
    let events_file = format!("{}/events.ndjson", root);
    let index_file = format!("{}/events.index.tsv", root);
    fs::create_dir_all(root).map_err(|err| format!("failed to create {}: {}", root, err))?;

    let seq = fs::read_to_string(&seq_file)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(0)
        + 1;
    write_text_atomic(&seq_file, &seq.to_string())?;

    let ts = current_utc_timestamp()?;
    let ts_epoch = current_epoch_seconds()?;
    let detail = parse_json_file(&values[7], "registry event detail file")?;
    let envelope = JsonValue::Object(BTreeMap::from([
        (
            "kind".to_string(),
            JsonValue::String("runtime-event".to_string()),
        ),
        (
            "version".to_string(),
            JsonValue::Number(JsonNumber::from_int(1)),
        ),
        (
            "payload".to_string(),
            JsonValue::Object(BTreeMap::from([
                ("runId".to_string(), JsonValue::String(values[2].clone())),
                ("attemptId".to_string(), nullable_string_value(&values[3])),
                ("workflowId".to_string(), nullable_string_value(&values[4])),
                ("taskId".to_string(), nullable_string_value(&values[5])),
                ("seq".to_string(), JsonValue::Number(JsonNumber::from_int(seq))),
                ("ts".to_string(), JsonValue::String(ts.clone())),
                ("state".to_string(), JsonValue::String(values[6].clone())),
                ("detail".to_string(), detail),
            ])),
        ),
    ]));
    validate_and_write_json(bundle_path, "runtime.registryEvent", "-", &envelope)?;

    let rendered = render_json_compact(&envelope);
    append_line(&events_file, &rendered)?;
    append_line(
        &index_file,
        &format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            seq,
            ts_epoch,
            ts,
            values[2],
            values[3],
            values[4],
            values[5],
            values[6],
            values[8],
            values[9]
        ),
    )?;
    write_shell_exports(
        &values[10],
        &[
            ("REGISTRY_APPEND_LAST_SEQ".to_string(), seq.to_string()),
            (
                "REGISTRY_APPEND_LAST_EVENT_JSON".to_string(),
                rendered.to_string(),
            ),
        ],
    )?;
    println!("OK: registry append");
    Ok(())
}

fn registry_replay_command(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err("usage: nixfied-kernel registry replay <root>".to_string());
    }

    let index_file = format!("{}/events.index.tsv", values[0]);
    if !Path::new(&index_file).exists() {
        println!("{{}}");
        return Ok(());
    }

    let content = read_text(&index_file)?;
    let mut replay = BTreeMap::new();
    for line in content.lines() {
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() < 8 {
            continue;
        }
        let workflow_id = parts[5];
        let task_id = parts[6];
        let state = parts[7];
        let key = if !task_id.is_empty() {
            format!("task:{}", task_id)
        } else if !workflow_id.is_empty() {
            format!("workflow:{}", workflow_id)
        } else {
            continue;
        };
        replay.insert(key, JsonValue::String(state.to_string()));
    }

    println!("{}", render_json_compact(&JsonValue::Object(replay)));
    Ok(())
}

fn summary_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "write" => summary_write_command(values),
        "render-human" => summary_render_human_command(values),
        other => Err(format!("unknown summary subcommand: {}", other)),
    }
}

fn summary_write_command(values: &[String]) -> Result<(), String> {
    if values.len() != 3 {
        return Err(
            "usage: nixfied-kernel summary write <bundle-file> <summary-file> <input-file>"
                .to_string(),
        );
    }

    let input = parse_json_file(&values[2], "summary input file")?;
    let envelope = if object_field(&input, "kind").is_some() && object_field(&input, "payload").is_some() {
        input
    } else {
        JsonValue::Object(BTreeMap::from([
            (
                "kind".to_string(),
                JsonValue::String("workflow-summary".to_string()),
            ),
            (
                "version".to_string(),
                JsonValue::Number(JsonNumber::from_int(1)),
            ),
            ("payload".to_string(), input),
        ]))
    };
    validate_and_write_json(&values[0], "runtime.summary", &values[1], &envelope)?;
    println!("OK: summary write");
    Ok(())
}

fn summary_render_human_command(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err("usage: nixfied-kernel summary render-human <summary-file>".to_string());
    }

    let summary = parse_json_file(&values[0], "summary file")?;
    let payload = object_field(&summary, "payload")
        .ok_or_else(|| "summary payload is missing".to_string())?;
    let steps = object_array(payload, "steps").unwrap_or(&[]);
    let counts = object_field(payload, "counts")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "summary counts are missing".to_string())?;
    let timing = object_field(payload, "timing")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "summary timing is missing".to_string())?;
    let total_duration = object_field(payload, "duration_seconds")
        .and_then(json_value_to_i64)
        .unwrap_or(0);
    let exit_code = object_field(payload, "exit_code")
        .and_then(json_value_to_i64)
        .unwrap_or(1);
    let skipped = counts
        .get("skipped")
        .and_then(json_value_to_i64)
        .unwrap_or(0);
    let parallel = timing
        .get("parallelism")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();

    println!();
    println!("------------------------------------------------------------");
    println!("Summary");
    println!("------------------------------------------------------------");
    println!("Source: {}", values[0]);
    for step in steps {
        let status = object_string(step, "status").unwrap_or("failed");
        let marker = match status {
            "passed" => "PASS",
            "skipped" => "SKIP",
            _ => "FAIL",
        };
        let name = object_string(step, "name").unwrap_or("unknown");
        let duration = object_field(step, "duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0);
        println!("  [{}] {} ({}s)", marker, name, duration);
    }
    println!("Total time: {}", format_duration_seconds(total_duration));
    println!(
        "INFO: Time breakdown setup={}s steps={}s teardown={}s accounted={}s untracked={}s",
        timing.get("setup_duration").and_then(json_value_to_i64).unwrap_or(0),
        timing.get("steps_duration").and_then(json_value_to_i64).unwrap_or(0),
        timing.get("teardown_duration").and_then(json_value_to_i64).unwrap_or(0),
        timing
            .get("accounted_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        timing
            .get("untracked_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0)
    );
    println!(
        "INFO: Parallelism max_workers={} peak_workers={} canceled_count={}",
        parallel
            .get("max_workers")
            .map(json_scalar_or_placeholder)
            .unwrap_or_else(|| "?".to_string()),
        parallel
            .get("peak_workers")
            .map(json_scalar_or_placeholder)
            .unwrap_or_else(|| "?".to_string()),
        parallel
            .get("canceled_count")
            .map(json_scalar_or_placeholder)
            .unwrap_or_else(|| "?".to_string())
    );
    if skipped > 0 {
        println!("INFO: SKIP: {} task(s) skipped", skipped);
    }
    if exit_code == 0 {
        println!("OK: Exit code: 0");
    } else {
        println!("ERROR: Exit code: {}", exit_code);
    }
    if let Some(parent) = Path::new(&values[0]).parent() {
        println!();
        println!("Artifacts: {}", parent.display());
    }
    println!("------------------------------------------------------------");
    Ok(())
}

fn validate_and_write_json(
    bundle_path: &str,
    contract_ref: &str,
    output_path: &str,
    value: &JsonValue,
) -> Result<(), String> {
    let bundle = parse_json_file(bundle_path, "validation bundle")?;
    validate_json_value_against_contract(&bundle, contract_ref, value)?;
    if output_path != "-" {
        write_text_atomic(output_path, &format!("{}\n", render_json_compact(value)))?;
    }
    Ok(())
}

fn parse_json_file(path: &str, label: &str) -> Result<JsonValue, String> {
    let text = read_text(path)?;
    parse_json(&text).map_err(|err| format!("{} {} is not valid JSON: {}", label, path, err))
}

fn run_record_history_entry(state: &str, at: &str) -> JsonValue {
    JsonValue::Object(BTreeMap::from([
        ("state".to_string(), JsonValue::String(state.to_string())),
        ("at".to_string(), JsonValue::String(at.to_string())),
    ]))
}

fn parse_bool_flag(value: &str) -> Result<bool, String> {
    match value {
        "1" | "true" | "TRUE" => Ok(true),
        "0" | "false" | "FALSE" => Ok(false),
        other => Err(format!("expected boolean flag, got {}", other)),
    }
}

fn nullable_string_value(value: &str) -> JsonValue {
    if value.is_empty() {
        JsonValue::Null
    } else {
        JsonValue::String(value.to_string())
    }
}

fn optional_string_value(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_optional_i64(value: &str, label: &str) -> Result<Option<i64>, String> {
    if value.is_empty() || value == "null" {
        Ok(None)
    } else {
        parse_i64_text(value, label).map(Some)
    }
}

fn append_line(path: &str, line: &str) -> Result<(), String> {
    let target = Path::new(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {}", parent.display(), err))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("failed to open {}: {}", path, err))?;
    file.write_all(line.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|err| format!("failed to append {}: {}", path, err))
}

fn current_utc_timestamp() -> Result<String, String> {
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .map_err(|err| format!("failed to run date: {}", err))?;
    if !output.status.success() {
        return Err("failed to compute UTC timestamp".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn current_epoch_seconds() -> Result<i64, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| format!("failed to compute epoch seconds: {}", err))?;
    Ok(i64::try_from(now.as_secs()).unwrap_or(i64::MAX))
}

fn print_json_scalar(value: &JsonValue) {
    match value {
        JsonValue::Null => println!(),
        JsonValue::Bool(value) => println!("{}", value),
        JsonValue::String(value) => println!("{}", value),
        JsonValue::Number(number) => println!("{}", number.raw),
        JsonValue::Array(_) | JsonValue::Object(_) => println!("{}", render_json_compact(value)),
    }
}

fn format_duration_seconds(seconds: i64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

fn json_scalar_or_placeholder(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "?".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::String(value) => value.clone(),
        JsonValue::Number(number) => number.raw.clone(),
        JsonValue::Array(_) | JsonValue::Object(_) => render_json_compact(value),
    }
}

fn adapter_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "decode" => adapter_decode_command(values),
        other => Err(format!("unknown adapter subcommand: {}", other)),
    }
}

fn adapter_decode_command(values: &[String]) -> Result<(), String> {
    let kind = values
        .first()
        .ok_or_else(|| "usage: nixfied-kernel adapter decode <kind> ...".to_string())?;
    match kind.as_str() {
        "supervisor-status" => adapter_decode_supervisor_status(&values[1..]),
        "helios-finalized-slot" => adapter_decode_helios_finalized_slot(&values[1..]),
        "helios-checkpoint-root" => adapter_decode_helios_checkpoint_root(&values[1..]),
        other => Err(format!("unknown adapter decode kind: {}", other)),
    }
}

fn adapter_decode_supervisor_status(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err(
            "usage: nixfied-kernel adapter decode supervisor-status <json-file>".to_string(),
        );
    }
    let payload = parse_json_file(&values[0], "supervisor payload")?;
    let rows = payload
        .as_array()
        .ok_or_else(|| "supervisor payload must be an array".to_string())?;
    for row in rows {
        let object = row
            .as_object()
            .ok_or_else(|| "supervisor process row must be an object".to_string())?;
        println!(
            "{}\t{}\t{}\t{}",
            object
                .get("name")
                .and_then(JsonValue::as_string)
                .unwrap_or(""),
            object
                .get("status")
                .and_then(JsonValue::as_string)
                .unwrap_or(""),
            object
                .get("is_running")
                .and_then(JsonValue::as_bool)
                .map(|value| value.to_string())
                .unwrap_or_default(),
            object
                .get("is_ready")
                .and_then(JsonValue::as_bool)
                .map(|value| value.to_string())
                .unwrap_or_default()
        );
    }
    Ok(())
}

fn adapter_decode_helios_finalized_slot(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err(
            "usage: nixfied-kernel adapter decode helios-finalized-slot <json-file>"
                .to_string(),
        );
    }
    let payload = parse_json_file(&values[0], "helios finalized payload")?;
    let selected = resolve_json_path(&payload, ".data.header.message.slot")
        .ok_or_else(|| "helios finalized slot is missing".to_string())?;
    let number = json_value_to_number_string(selected)
        .ok_or_else(|| "helios finalized slot is not numeric".to_string())?;
    println!("{}", number);
    Ok(())
}

fn adapter_decode_helios_checkpoint_root(values: &[String]) -> Result<(), String> {
    if values.is_empty() || values.len() > 2 {
        return Err(
            "usage: nixfied-kernel adapter decode helios-checkpoint-root <json-file> [--empty-ok]"
                .to_string(),
        );
    }
    let empty_ok = values.get(1).map(|value| value == "--empty-ok").unwrap_or(false);
    let payload = parse_json_file(&values[0], "helios checkpoint payload")?;
    let selected = resolve_json_path(&payload, ".data.root");
    match selected.and_then(JsonValue::as_string) {
        Some(value) => {
            println!("{}", value);
            Ok(())
        }
        None if empty_ok => Ok(()),
        None => Err("helios checkpoint root is missing".to_string()),
    }
}

fn probe_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "evaluate" => probe_evaluate_command(values),
        other => Err(format!("unknown probe subcommand: {}", other)),
    }
}

fn probe_evaluate_command(values: &[String]) -> Result<(), String> {
    let plan_path = values
        .first()
        .ok_or_else(|| "usage: nixfied-kernel probe evaluate <plan-file> <payload-file> [export-file]".to_string())?;
    let payload_path = values
        .get(1)
        .ok_or_else(|| "usage: nixfied-kernel probe evaluate <plan-file> <payload-file> [export-file]".to_string())?;
    if values.len() > 3 {
        return Err(
            "usage: nixfied-kernel probe evaluate <plan-file> <payload-file> [export-file]"
                .to_string(),
        );
    }

    let export_path = values.get(2).map(|value| value.as_str());
    let plan = load_probe_plan(plan_path)?;
    let payload = parse_json_file(payload_path, "probe payload")?;
    let result = resolve_json_path(&payload, ".result");
    let mut exports = Vec::new();

    match plan.probe_kind.as_str() {
        "jsonrpc-result-present" => {
            if matches!(result, Some(JsonValue::Null) | None) {
                return Err("probe result is missing".to_string());
            }
        }
        "jsonrpc-result-hex" => {
            let value = result
                .and_then(JsonValue::as_string)
                .ok_or_else(|| "probe result must be a hex string".to_string())?;
            if !is_hex_prefixed(value) {
                return Err(format!("probe result must be hex, got {}", value));
            }
            let export_var = plan
                .export_var
                .clone()
                .ok_or_else(|| "probe plan jsonrpc-result-hex requires exportVar".to_string())?;
            exports.push((export_var, value.to_string()));
        }
        "jsonrpc-result-compact" => {
            let value = result.ok_or_else(|| "probe result is missing".to_string())?;
            if matches!(value, JsonValue::Null) {
                return Err("probe result is missing".to_string());
            }
            let export_var = plan
                .export_var
                .clone()
                .ok_or_else(|| "probe plan jsonrpc-result-compact requires exportVar".to_string())?;
            exports.push((export_var, render_json_compact(value)));
        }
        "jsonrpc-result-bool-false" => match result {
            Some(JsonValue::Bool(false)) => {}
            _ => return Err("probe result must be false".to_string()),
        },
        other => return Err(format!("unknown probe evaluate kind: {}", other)),
    }

    if let Some(export_path) = export_path {
        write_shell_exports(export_path, &exports)?;
    } else if !exports.is_empty() {
        return Err("probe evaluate requires export-file when plan emits exports".to_string());
    }

    println!("OK: probe evaluate kind={}", plan.probe_kind);
    Ok(())
}

fn machine_output_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "run" => machine_output_run_command(values),
        other => Err(format!("unknown machine-output subcommand: {}", other)),
    }
}

fn machine_output_run_command(values: &[String]) -> Result<(), String> {
    let plan_path = values
        .first()
        .ok_or_else(|| "usage: nixfied-kernel machine-output run <plan-file> [-- <args...>]".to_string())?;
    let plan = load_machine_output_plan(plan_path)?;
    let remaining = values[1..].to_vec();
    let user_args = strip_passthrough_separator(&remaining).to_vec();
    let work_dir = create_temp_dir("nixfied-machine-output")?;

    for (index, setup_program) in plan.setup_programs.iter().enumerate() {
        let output = run_captured_program(setup_program, &[], &[])?;
        if output.status.success() {
            render_captured_logs(
                "INFO",
                &format!("setup app {}", index + 1),
                &output,
            );
        } else {
            render_captured_logs(
                "ERROR",
                &format!("setup app {}", index + 1),
                &output,
            );
            machine_output_fail(
                &plan,
                "setup",
                "machine-output-setup-failed",
                &format!("setup app {} failed", index + 1),
                "",
                output.status.code().unwrap_or(1),
            );
        }
    }

    let payload_file = format!("{}/payload.json", work_dir);
    let mut target_args = plan.target_args.clone();
    target_args.extend(user_args.iter().cloned());
    let output = run_captured_program(
        &plan.target_program,
        &target_args,
        &[("NIXFIED_MACHINE_OUTPUT_FILE".to_string(), payload_file.clone())],
    )?;
    if output.status.success() {
        render_captured_logs("INFO", "target app", &output);
    } else {
        render_captured_logs("ERROR", "target app", &output);
        machine_output_fail(
            &plan,
            "target",
            "machine-output-target-failed",
            &format!("target app '{}' failed", plan.target_app_id),
            &plan.target_app_id,
            output.status.code().unwrap_or(1),
        );
    }

    let payload_text = match fs::read_to_string(&payload_file) {
        Ok(text) if !text.trim().is_empty() => text,
        _ => {
            machine_output_fail(
                &plan,
                "validation",
                "machine-output-validation-failed",
                &format!(
                    "target app '{}' did not write machine payload to declared file",
                    plan.target_app_id
                ),
                &plan.target_app_id,
                1,
            );
        }
    };
    let payload = parse_json(&payload_text).unwrap_or_else(|_| {
        machine_output_fail(
            &plan,
            "validation",
            "machine-output-validation-failed",
            &format!(
                "target app '{}' did not satisfy contract '{}'",
                plan.target_app_id, plan.contract_ref
            ),
            &plan.target_app_id,
            1,
        );
    });
    let bundle = parse_json_file(&plan.bundle_file, "machine-output validation bundle")
        .unwrap_or_else(|err| panic!("{}", err));
    if let Err(err) = validate_json_value_against_contract(&bundle, &plan.contract_ref, &payload) {
        eprintln!("ERROR: {}", err);
        machine_output_fail(
            &plan,
            "validation",
            "machine-output-validation-failed",
            &format!(
                "target app '{}' did not satisfy contract '{}'",
                plan.target_app_id, plan.contract_ref
            ),
            &plan.target_app_id,
            1,
        );
    }

    for (index, teardown_program) in plan.teardown_programs.iter().enumerate() {
        let output = run_captured_program(teardown_program, &[], &[])?;
        if output.status.success() {
            render_captured_logs(
                "INFO",
                &format!("teardown app {}", index + 1),
                &output,
            );
        } else {
            render_captured_logs(
                "ERROR",
                &format!("teardown app {}", index + 1),
                &output,
            );
            machine_output_fail(
                &plan,
                "teardown",
                "machine-output-teardown-failed",
                &format!("teardown app {} failed", index + 1),
                "",
                output.status.code().unwrap_or(1),
            );
        }
    }

    print!("{}", payload_text);
    Ok(())
}

fn load_machine_output_plan(path: &str) -> Result<MachineOutputPlan, String> {
    let value = parse_json_file(path, "machine-output plan")?;
    Ok(MachineOutputPlan {
        app_id: required_string_field(&value, "appId", "machine-output plan")?.to_string(),
        target_app_id: required_string_field(&value, "targetAppId", "machine-output plan")?
            .to_string(),
        contract_ref: required_string_field(&value, "contractRef", "machine-output plan")?
            .to_string(),
        bundle_file: required_string_field(&value, "bundleFile", "machine-output plan")?
            .to_string(),
        target_program: required_string_field(&value, "targetProgram", "machine-output plan")?
            .to_string(),
        setup_programs: array_strings(&value, "setupPrograms"),
        teardown_programs: array_strings(&value, "teardownPrograms"),
        target_args: array_strings(&value, "targetArgs"),
    })
}

fn load_probe_plan(path: &str) -> Result<ProbePlan, String> {
    let value = parse_json_file(path, "probe plan")?;
    Ok(ProbePlan {
        probe_kind: required_string_field(&value, "probeKind", "probe plan")?.to_string(),
        export_var: object_string(&value, "exportVar").map(|value| value.to_string()),
    })
}

fn create_temp_dir(prefix: &str) -> Result<String, String> {
    let base = env::temp_dir();
    let candidate = base.join(format!(
        "{}.{}.{}",
        prefix,
        process::id(),
        current_epoch_seconds()?
    ));
    fs::create_dir_all(&candidate)
        .map_err(|err| format!("failed to create {}: {}", candidate.display(), err))?;
    Ok(candidate.display().to_string())
}

fn run_captured_program(
    program: &str,
    args: &[String],
    envs: &[(String, String)],
) -> Result<std::process::Output, String> {
    let mut command = Command::new(program);
    command.args(args);
    command.stdin(Stdio::null());
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .output()
        .map_err(|err| format!("failed to run {}: {}", program, err))
}

fn render_captured_logs(level: &str, label: &str, output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.is_empty() {
        eprintln!("{}: {} stdout:", level, label);
        eprint!("{}", stdout);
    }
    if !stderr.is_empty() {
        eprintln!("{}: {} stderr:", level, label);
        eprint!("{}", stderr);
    }
}

fn machine_output_fail(
    plan: &MachineOutputPlan,
    stage: &str,
    code: &str,
    message: &str,
    failed_app_id: &str,
    exit_code: i32,
) -> ! {
    let payload = JsonValue::Object(BTreeMap::from([
        ("ok".to_string(), JsonValue::Bool(false)),
        ("appId".to_string(), JsonValue::String(plan.app_id.clone())),
        (
            "targetAppId".to_string(),
            JsonValue::String(plan.target_app_id.clone()),
        ),
        ("stage".to_string(), JsonValue::String(stage.to_string())),
        ("code".to_string(), JsonValue::String(code.to_string())),
        ("message".to_string(), JsonValue::String(message.to_string())),
        (
            "failedAppId".to_string(),
            nullable_string_value(failed_app_id),
        ),
        (
            "contractRef".to_string(),
            nullable_string_value(&plan.contract_ref),
        ),
        (
            "validator".to_string(),
            if stage == "validation" {
                JsonValue::String("nixfied-kernel".to_string())
            } else {
                JsonValue::Null
            },
        ),
        (
            "exitCode".to_string(),
            JsonValue::Number(JsonNumber::from_int(exit_code as i64)),
        ),
    ]));
    println!("{}", render_json_compact(&payload));
    process::exit(exit_code.max(1));
}

fn is_hex_prefixed(value: &str) -> bool {
    value.len() >= 3
        && value.starts_with("0x")
        && value
            .chars()
            .skip(2)
            .all(|ch| ch.is_ascii_hexdigit())
}

fn validate_json_command(input_path: &str) -> Result<(), String> {
    let input = read_text(input_path)?;
    parse_json(&input).map_err(|err| format!("input {} is not valid JSON: {}", input_path, err))?;
    println!("OK: validate-json");
    Ok(())
}

fn json_length_command(input_path: &str) -> Result<(), String> {
    let input = read_text(input_path)?;
    let value = parse_json(&input)
        .map_err(|err| format!("input {} is not valid JSON: {}", input_path, err))?;
    let length = match value {
        JsonValue::Array(values) => values.len(),
        JsonValue::Object(values) => values.len(),
        other => {
            return Err(format!(
                "input {} must be an array or object for json-length (found {})",
                input_path,
                value_type(&other)
            ))
        }
    };
    println!("{}", length);
    Ok(())
}

fn query_json_command(
    input_path: &str,
    path_expr: &str,
    raw: bool,
    empty_ok: bool,
    tonumber: bool,
) -> Result<(), String> {
    let input = read_text(input_path)?;
    let value = parse_json(&input)
        .map_err(|err| format!("input {} is not valid JSON: {}", input_path, err))?;

    let mut empty_ok = empty_ok;
    let mut path_expr = path_expr.trim();
    if let Some(stripped) = path_expr.strip_suffix("// empty") {
        empty_ok = true;
        path_expr = stripped.trim_end();
    }

    let selected = resolve_json_path(&value, path_expr);
    let Some(selected) = selected else {
        if empty_ok {
            return Ok(());
        }
        println!("null");
        return Ok(());
    };

    if empty_ok && matches!(selected, JsonValue::Null) {
        return Ok(());
    }

    if tonumber {
        let number = json_value_to_number_string(selected).ok_or_else(|| {
            format!(
                "query-json path {} in {} cannot be converted to a number",
                path_expr, input_path
            )
        })?;
        println!("{}", number);
        return Ok(());
    }

    if raw {
        match selected {
            JsonValue::Null => println!("null"),
            JsonValue::Bool(value) => println!("{}", value),
            JsonValue::String(value) => println!("{}", value),
            JsonValue::Number(value) => println!("{}", value.raw),
            JsonValue::Array(_) | JsonValue::Object(_) => {
                println!("{}", render_json_compact(selected))
            }
        }
    } else {
        println!("{}", render_json_compact(selected));
    }

    Ok(())
}

fn read_text(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|err| format!("failed to read stdin: {}", err))?;
        return Ok(buffer);
    }

    fs::read_to_string(path).map_err(|err| format!("failed to read {}: {}", path, err))
}

#[derive(Clone, Debug)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(JsonNumber),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

#[derive(Clone, Debug)]
struct JsonNumber {
    raw: String,
    integer: bool,
    int_value: Option<i128>,
    float_value: f64,
}

impl JsonValue {
    fn as_object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(value) => Some(value),
            _ => None,
        }
    }

    fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(value) => Some(value),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(value) => Some(value.as_slice()),
            _ => None,
        }
    }

    fn as_array_mut(&mut self) -> Option<&mut Vec<JsonValue>> {
        match self {
            JsonValue::Array(value) => Some(value),
            _ => None,
        }
    }

    fn as_string(&self) -> Option<&str> {
        match self {
            JsonValue::String(value) => Some(value.as_str()),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(value) => Some(*value),
            _ => None,
        }
    }

    fn as_number(&self) -> Option<&JsonNumber> {
        match self {
            JsonValue::Number(value) => Some(value),
            _ => None,
        }
    }
}

impl JsonNumber {
    fn from_int(value: i64) -> Self {
        Self {
            raw: value.to_string(),
            integer: true,
            int_value: Some(value as i128),
            float_value: value as f64,
        }
    }
}

fn parse_json(input: &str) -> Result<JsonValue, String> {
    let mut parser = Parser::new(input);
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.peek().is_some() {
        return Err(parser.error("trailing content after JSON value"));
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Parser {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn error(&self, message: impl Into<String>) -> String {
        format!(
            "line {} column {}: {}",
            self.line,
            self.col,
            message.into()
        )
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\n' | '\t' | '\r')) {
            let _ = self.next();
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_whitespace();
        match self.peek() {
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"') => Ok(JsonValue::String(self.parse_string()?)),
            Some('t') => {
                self.expect_keyword("true")?;
                Ok(JsonValue::Bool(true))
            }
            Some('f') => {
                self.expect_keyword("false")?;
                Ok(JsonValue::Bool(false))
            }
            Some('n') => {
                self.expect_keyword("null")?;
                Ok(JsonValue::Null)
            }
            Some('-') | Some('0'..='9') => Ok(JsonValue::Number(self.parse_number()?)),
            Some(ch) => Err(self.error(format!("unexpected character '{}'", ch))),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), String> {
        for expected in keyword.chars() {
            match self.next() {
                Some(actual) if actual == expected => {}
                Some(actual) => {
                    return Err(self.error(format!(
                        "expected keyword {}, got character '{}'",
                        keyword, actual
                    )))
                }
                None => return Err(self.error(format!("expected keyword {}", keyword))),
            }
        }
        Ok(())
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect_char('"')?;
        let mut out = String::new();
        loop {
            let ch = self
                .next()
                .ok_or_else(|| self.error("unterminated string literal"))?;
            match ch {
                '"' => break,
                '\\' => out.push(self.parse_escape_sequence()?),
                '\u{0000}'..='\u{001F}' => {
                    return Err(self.error("unescaped control character in string"))
                }
                other => out.push(other),
            }
        }
        Ok(out)
    }

    fn parse_escape_sequence(&mut self) -> Result<char, String> {
        match self.next() {
            Some('"') => Ok('"'),
            Some('\\') => Ok('\\'),
            Some('/') => Ok('/'),
            Some('b') => Ok('\u{0008}'),
            Some('f') => Ok('\u{000c}'),
            Some('n') => Ok('\n'),
            Some('r') => Ok('\r'),
            Some('t') => Ok('\t'),
            Some('u') => self.parse_unicode_escape(),
            Some(other) => Err(self.error(format!("invalid escape sequence '\\{}'", other))),
            None => Err(self.error("unterminated escape sequence")),
        }
    }

    fn parse_unicode_escape(&mut self) -> Result<char, String> {
        let first = self.parse_hex_quad()?;
        if (0xD800..=0xDBFF).contains(&first) {
            self.expect_char('\\')?;
            self.expect_char('u')?;
            let second = self.parse_hex_quad()?;
            if !(0xDC00..=0xDFFF).contains(&second) {
                return Err(self.error("invalid UTF-16 surrogate pair"));
            }
            let codepoint = 0x10000
                + ((((first - 0xD800) as u32) << 10) | ((second - 0xDC00) as u32));
            return char::from_u32(codepoint)
                .ok_or_else(|| self.error("invalid Unicode escape sequence"));
        }
        if (0xDC00..=0xDFFF).contains(&first) {
            return Err(self.error("unexpected low surrogate in Unicode escape"));
        }
        char::from_u32(first as u32).ok_or_else(|| self.error("invalid Unicode escape sequence"))
    }

    fn parse_hex_quad(&mut self) -> Result<u16, String> {
        let mut value = 0u16;
        for _ in 0..4 {
            let ch = self
                .next()
                .ok_or_else(|| self.error("unexpected end of Unicode escape"))?;
            let digit = ch
                .to_digit(16)
                .ok_or_else(|| self.error("invalid hex digit in Unicode escape"))?;
            value = (value << 4) | digit as u16;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<JsonNumber, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            let _ = self.next();
        }

        match self.peek() {
            Some('0') => {
                let _ = self.next();
                if matches!(self.peek(), Some('0'..='9')) {
                    return Err(self.error("leading zeros are not allowed in JSON numbers"));
                }
            }
            Some('1'..='9') => {
                let _ = self.next();
                while matches!(self.peek(), Some('0'..='9')) {
                    let _ = self.next();
                }
            }
            _ => return Err(self.error("invalid JSON number")),
        }

        if self.peek() == Some('.') {
            let _ = self.next();
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(self.error("fractional part requires digits"));
            }
            while matches!(self.peek(), Some('0'..='9')) {
                let _ = self.next();
            }
        }

        if matches!(self.peek(), Some('e' | 'E')) {
            let _ = self.next();
            if matches!(self.peek(), Some('+' | '-')) {
                let _ = self.next();
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(self.error("exponent requires digits"));
            }
            while matches!(self.peek(), Some('0'..='9')) {
                let _ = self.next();
            }
        }

        let raw: String = self.chars[start..self.pos].iter().collect();
        let integer = !raw.contains('.') && !raw.contains('e') && !raw.contains('E');
        let float_value = raw
            .parse::<f64>()
            .map_err(|err| self.error(format!("invalid JSON number: {}", err)))?;
        let int_value = if integer {
            raw.parse::<i128>().ok()
        } else {
            None
        };

        Ok(JsonNumber {
            raw,
            integer,
            int_value,
            float_value,
        })
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.expect_char('[')?;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.peek() == Some(']') {
            let _ = self.next();
            return Ok(JsonValue::Array(values));
        }

        loop {
            values.push(self.parse_value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    let _ = self.next();
                    self.skip_whitespace();
                }
                Some(']') => {
                    let _ = self.next();
                    break;
                }
                Some(other) => {
                    return Err(self.error(format!(
                        "expected ',' or ']' after array item, found '{}'",
                        other
                    )))
                }
                None => return Err(self.error("unterminated array")),
            }
        }

        Ok(JsonValue::Array(values))
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.expect_char('{')?;
        self.skip_whitespace();
        let mut values = BTreeMap::new();
        if self.peek() == Some('}') {
            let _ = self.next();
            return Ok(JsonValue::Object(values));
        }

        loop {
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect_char(':')?;
            let value = self.parse_value()?;
            values.insert(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    let _ = self.next();
                    self.skip_whitespace();
                }
                Some('}') => {
                    let _ = self.next();
                    break;
                }
                Some(other) => {
                    return Err(self.error(format!(
                        "expected ',' or '}}' after object item, found '{}'",
                        other
                    )))
                }
                None => return Err(self.error("unterminated object")),
            }
        }

        Ok(JsonValue::Object(values))
    }

    fn expect_char(&mut self, expected: char) -> Result<(), String> {
        match self.next() {
            Some(actual) if actual == expected => Ok(()),
            Some(actual) => Err(self.error(format!(
                "expected '{}', found '{}'",
                expected, actual
            ))),
            None => Err(self.error(format!("expected '{}'", expected))),
        }
    }
}

#[derive(Clone, Debug)]
enum PathSegment {
    Field(String),
    Index(usize),
}

fn format_path(path: &[PathSegment]) -> String {
    let mut out = String::from("$");
    for segment in path {
        match segment {
            PathSegment::Field(field) => {
                if is_identifier(field) {
                    out.push('.');
                    out.push_str(field);
                } else {
                    out.push_str("['");
                    out.push_str(&escape_path_key(field));
                    out.push_str("']");
                }
            }
            PathSegment::Index(index) => {
                out.push('[');
                out.push_str(&index.to_string());
                out.push(']');
            }
        }
    }
    out
}

fn escape_path_key(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            other => out.push(other),
        }
    }
    out
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

struct ValidationContext<'a> {
    root: &'a JsonValue,
    definition_maps: Vec<&'a BTreeMap<String, JsonValue>>,
}

impl<'a> ValidationContext<'a> {
    fn new(root: &'a JsonValue) -> Self {
        let mut definition_maps = Vec::new();
        if let Some(object) = root.as_object() {
            if let Some(definitions) = object.get("definitions").and_then(JsonValue::as_object) {
                definition_maps.push(definitions);
            }
            if let Some(bundle) = object.get("bundle").and_then(JsonValue::as_object) {
                if let Some(definitions) = bundle.get("definitions").and_then(JsonValue::as_object)
                {
                    definition_maps.push(definitions);
                }
            }
            if let Some(validation) = object.get("validation").and_then(JsonValue::as_object) {
                if let Some(definitions) = validation.get("definitions").and_then(JsonValue::as_object)
                {
                    definition_maps.push(definitions);
                }
            }
        }

        Self {
            root,
            definition_maps,
        }
    }

    fn resolve_contract_ref(&self, contract_ref: &str) -> Result<&'a JsonValue, String> {
        if contract_ref.starts_with('#') || contract_ref.starts_with('/') {
            if let Some(value) = resolve_json_pointer(self.root, contract_ref) {
                return Ok(value);
            }
        }

        let mut candidates = Vec::new();
        if !candidates.iter().any(|entry| entry == contract_ref) {
            candidates.push(contract_ref.to_string());
        }
        add_candidate(&mut candidates, contract_ref);
        if let Some(stripped) = contract_ref.strip_prefix('#') {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.strip_prefix("#/") {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.strip_prefix('/') {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.strip_prefix("#/definitions/") {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.strip_prefix("/definitions/") {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.strip_prefix("definitions/") {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.rsplit('/').next() {
            add_candidate(&mut candidates, stripped);
        }

        for candidate in candidates {
            for definitions in &self.definition_maps {
                if let Some(value) = definitions.get(&candidate) {
                    return Ok(value);
                }
            }
        }

        let known = self
            .definition_maps
            .iter()
            .flat_map(|definitions| definitions.keys().cloned())
            .collect::<Vec<_>>()
            .join(", ");

        Err(if known.is_empty() {
            format!("reference {} was not found", contract_ref)
        } else {
            format!(
                "reference {} was not found; known definitions: {}",
                contract_ref, known
            )
        })
    }
}

fn add_candidate(candidates: &mut Vec<String>, candidate: &str) {
    let value = candidate.trim_start_matches('#').trim_start_matches('/');
    if !value.is_empty() && !candidates.iter().any(|entry| entry == value) {
        candidates.push(value.to_string());
    }
}

fn resolve_json_pointer<'a>(value: &'a JsonValue, pointer: &str) -> Option<&'a JsonValue> {
    let mut current = value;
    let pointer = pointer.strip_prefix('#').unwrap_or(pointer);
    if pointer.is_empty() {
        return Some(current);
    }
    let pointer = pointer.strip_prefix('/').unwrap_or(pointer);
    if pointer.is_empty() {
        return Some(current);
    }

    for token in pointer.split('/') {
        let token = token.replace("~1", "/").replace("~0", "~");
        current = match current {
            JsonValue::Object(map) => map.get(&token)?,
            JsonValue::Array(array) => {
                let index = token.parse::<usize>().ok()?;
                array.get(index)?
            }
            _ => return None,
        };
    }

    Some(current)
}

fn validate_schema(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let current = unwrap_schema_reference(context, schema, ref_stack)?;
    let kind = schema_kind(current);

    match kind.as_deref() {
        Some("any") | Some("anything") | Some("unknown") => Ok(()),
        Some("bool") | Some("boolean") => validate_bool(value, path),
        Some("null") => validate_null(value, path),
        Some("string") => validate_string(current, value, path),
        Some("integer") | Some("int") | Some("int64") => validate_integer(current, value, path),
        Some("number") | Some("float") | Some("double") => validate_number(current, value, path),
        Some("literal") | Some("const") => validate_literal(current, value, path),
        Some("enum") => validate_enum(current, value, path),
        Some("list") | Some("array") => validate_list(context, current, value, path, ref_stack),
        Some("map") => validate_map(context, current, value, path, ref_stack),
        Some("record") => validate_record(context, current, value, path, ref_stack),
        Some("union") | Some("oneof") | Some("anyof") => {
            validate_union(context, current, value, path, ref_stack)
        }
        Some("taggedunion") => validate_tagged_union(context, current, value, path, ref_stack),
        Some("object") => validate_object_alias(context, current, value, path, ref_stack),
        Some(other) => Err(error_at(path, format!("unsupported schema kind '{}'", other))),
        None => {
            if object_field(current, "schema").is_some() {
                validate_schema(
                    context,
                    object_field(current, "schema").unwrap(),
                    value,
                    path,
                    ref_stack,
                )
            } else {
                Err(error_at(path, "schema is missing a kind".to_string()))
            }
        }
    }
}

fn unwrap_schema_reference<'a>(
    context: &ValidationContext<'a>,
    schema: &'a JsonValue,
    ref_stack: &mut Vec<String>,
) -> Result<&'a JsonValue, String> {
    let mut current = schema;
    let mut pushed = 0usize;
    loop {
        let kind = schema_kind(current);
        let reference = if matches!(kind.as_deref(), Some("ref")) || kind.is_none() {
            object_string(current, "ref")
                .or_else(|| object_string(current, "name"))
                .map(|value| value.to_string())
        } else {
            None
        };

        let Some(reference) = reference else {
            for _ in 0..pushed {
                let _ = ref_stack.pop();
            }
            return Ok(current);
        };

        if ref_stack.iter().any(|seen| seen == &reference) {
            for _ in 0..pushed {
                let _ = ref_stack.pop();
            }
            return Err(format!("cyclic reference detected at {}", reference));
        }

        ref_stack.push(reference.clone());
        pushed += 1;
        match context.resolve_contract_ref(&reference) {
            Ok(next) => current = next,
            Err(err) => {
                for _ in 0..pushed {
                    let _ = ref_stack.pop();
                }
                return Err(err);
            }
        }
    }
}

fn error_at(path: &[PathSegment], message: impl Into<String>) -> String {
    format!("path={} {}", format_path(path), message.into())
}

fn validate_bool(value: &JsonValue, path: &[PathSegment]) -> Result<(), String> {
    match value {
        JsonValue::Bool(_) => Ok(()),
        other => Err(error_at(path, format!("expected bool, found {}", value_type(other)))),
    }
}

fn validate_null(value: &JsonValue, path: &[PathSegment]) -> Result<(), String> {
    match value {
        JsonValue::Null => Ok(()),
        other => Err(error_at(path, format!("expected null, found {}", value_type(other)))),
    }
}

fn validate_string(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let string = match value {
        JsonValue::String(value) => value,
        other => return Err(error_at(path, format!("expected string, found {}", value_type(other)))),
    };

    if let Some(min_length) = schema_number(schema, "minLength") {
        if (string.chars().count() as f64) < min_length {
            return Err(error_at(
                path,
                format!("string is shorter than minLength {}", min_length),
            ));
        }
    }

    if let Some(max_length) = schema_number(schema, "maxLength") {
        if (string.chars().count() as f64) > max_length {
            return Err(error_at(
                path,
                format!("string is longer than maxLength {}", max_length),
            ));
        }
    }

    if let Some(patterns) = schema_patterns(schema) {
        for pattern in patterns {
            let compiled = SimplePattern::compile(&pattern).map_err(|err| {
                error_at(path, format!("invalid string pattern {}: {}", pattern, err))
            })?;
            if !compiled.matches(string) {
                return Err(error_at(
                    path,
                    format!("string does not match pattern {}", pattern),
                ));
            }
        }
    }

    Ok(())
}

fn validate_integer(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let number = match value {
        JsonValue::Number(value) if value.integer => value,
        JsonValue::Number(other) => {
            return Err(error_at(
                path,
                format!("expected integer, found number {}", other.raw),
            ))
        }
        other => return Err(error_at(path, format!("expected integer, found {}", value_type(other)))),
    };

    validate_numeric_bounds(schema, number.float_value, path)?;
    Ok(())
}

fn validate_number(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let number = match value {
        JsonValue::Number(value) => value,
        other => return Err(error_at(path, format!("expected number, found {}", value_type(other)))),
    };

    validate_numeric_bounds(schema, number.float_value, path)?;
    Ok(())
}

fn validate_literal(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let expected = object_field(schema, "value")
        .or_else(|| object_field(schema, "literal"))
        .or_else(|| object_field(schema, "const"));

    let Some(expected) = expected else {
        return Err(error_at(path, "literal schema is missing a value".to_string()));
    };

    if json_equal(expected, value) {
        Ok(())
    } else {
        Err(error_at(
            path,
            format!(
                "expected literal {}, found {}",
                json_preview(expected),
                json_preview(value)
            ),
        ))
    }
}

fn validate_enum(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let Some(values) = schema_enum_values(schema) else {
        return Err(error_at(path, "enum schema is missing values".to_string()));
    };

    if values.iter().any(|candidate| json_equal(candidate, value)) {
        Ok(())
    } else {
        Err(error_at(
            path,
            format!("value {} is not in enum", json_preview(value)),
        ))
    }
}

fn validate_list(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let items = match value {
        JsonValue::Array(values) => values,
        other => return Err(error_at(path, format!("expected array, found {}", value_type(other)))),
    };

    if let Some(tuple_items) = object_array(schema, "items") {
        if items.len() < tuple_items.len() {
            return Err(error_at(
                path,
                format!(
                    "array has {} items but schema requires {}",
                    items.len(),
                    tuple_items.len()
                ),
            ));
        }

        for (index, tuple_schema) in tuple_items.iter().enumerate() {
            path.push(PathSegment::Index(index));
            let result = validate_schema(context, tuple_schema, &items[index], path, ref_stack);
            path.pop();
            result?;
        }

        if items.len() > tuple_items.len() {
            if let Some(rest_schema) = object_field(schema, "rest")
                .or_else(|| object_field(schema, "additionalItems"))
            {
                for (index, item) in items.iter().enumerate().skip(tuple_items.len()) {
                    path.push(PathSegment::Index(index));
                    let result =
                        validate_schema(context, rest_schema, item, path, ref_stack);
                    path.pop();
                    result?;
                }
            } else {
                return Err(error_at(
                    path,
                    format!(
                        "array has {} items but schema only allows {}",
                        items.len(),
                        tuple_items.len()
                    ),
                ));
            }
        }
    } else if let Some(item_schema) = object_field(schema, "elem")
        .or_else(|| object_field(schema, "item"))
        .or_else(|| object_field(schema, "element"))
        .or_else(|| object_field(schema, "schema"))
    {
        for (index, item) in items.iter().enumerate() {
            path.push(PathSegment::Index(index));
            let result = validate_schema(context, item_schema, item, path, ref_stack);
            path.pop();
            result?;
        }
    }

    if let Some(min_items) = schema_number(schema, "minItems") {
        if (items.len() as f64) < min_items {
            return Err(error_at(
                path,
                format!("array has fewer than minItems {}", min_items),
            ));
        }
    }

    if let Some(max_items) = schema_number(schema, "maxItems") {
        if (items.len() as f64) > max_items {
            return Err(error_at(
                path,
                format!("array has more than maxItems {}", max_items),
            ));
        }
    }

    if object_bool(schema, "uniqueItems").unwrap_or(false) {
        for left in 0..items.len() {
            for right in (left + 1)..items.len() {
                if json_equal(&items[left], &items[right]) {
                    return Err(error_at(path, "array items are not unique".to_string()));
                }
            }
        }
    }

    Ok(())
}

fn validate_map(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let object = match value {
        JsonValue::Object(value) => value,
        other => return Err(error_at(path, format!("expected object, found {}", value_type(other)))),
    };

    let value_schema = object_field(schema, "value")
        .or_else(|| object_field(schema, "valueSchema"))
        .or_else(|| object_field(schema, "schema"));

    let key_pattern = object_string(schema, "keyPattern")
        .or_else(|| object_string(schema, "pattern"))
        .or_else(|| object_string(schema, "keyRegex"));

    let compiled_key_pattern = if let Some(pattern) = key_pattern {
        Some(
            SimplePattern::compile(pattern)
                .map_err(|err| error_at(path, format!("invalid key pattern {}: {}", pattern, err)))?,
        )
    } else {
        None
    };

    for (key, item) in object {
        if let Some(pattern) = &compiled_key_pattern {
            if !pattern.matches(key) {
                return Err(error_at(path, format!("key {} does not match key pattern", key)));
            }
        }

        if let Some(item_schema) = value_schema {
            path.push(PathSegment::Field(key.clone()));
            let result = validate_schema(context, item_schema, item, path, ref_stack);
            path.pop();
            result?;
        }
    }

    Ok(())
}

fn validate_record(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let object = match value {
        JsonValue::Object(value) => value,
        other => return Err(error_at(path, format!("expected object, found {}", value_type(other)))),
    };

    let (fields, closed) = record_fields(schema)?;

    for (field_name, field_spec) in &fields {
        match object.get(field_name) {
            Some(item) => {
                path.push(PathSegment::Field(field_name.clone()));
                let result = validate_schema(context, field_spec.schema, item, path, ref_stack);
                path.pop();
                result?;
            }
            None if field_spec.required => {
                return Err(error_at(
                    path,
                    format!("missing required field {}", field_name),
                ));
            }
            None => {}
        }
    }

    if closed {
        for key in object.keys() {
            if !fields.contains_key(key) {
                return Err(error_at(path, format!("unknown field {}", key)));
            }
        }
    }

    Ok(())
}

fn validate_object_alias(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    if has_record_shape(schema) {
        validate_record(context, schema, value, path, ref_stack)
    } else {
        validate_map(context, schema, value, path, ref_stack)
    }
}

fn validate_union(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let variants = schema_variants(schema).ok_or_else(|| {
        error_at(path, "union schema is missing variants".to_string())
    })?;

    let mut last_error = None;
    for variant in variants {
        let mut candidate_path = path.clone();
        match validate_schema(context, variant, value, &mut candidate_path, ref_stack) {
            Ok(()) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
    }

    Err(error_at(
        path,
        format!(
            "value did not match any union variant{}",
            last_error
                .as_ref()
                .map(|err| format!("; last error: {}", err))
                .unwrap_or_default()
        ),
    ))
}

fn validate_tagged_union(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let object = match value {
        JsonValue::Object(value) => value,
        other => return Err(error_at(path, format!("expected object, found {}", value_type(other)))),
    };

    let tag_field = object_string(schema, "tag")
        .or_else(|| object_string(schema, "tagField"))
        .or_else(|| object_string(schema, "discriminator"))
        .unwrap_or("tag");
    let tag_value = object.get(tag_field).ok_or_else(|| {
        error_at(path, format!("missing tagged union field {}", tag_field))
    })?;

    let tag_key = scalar_key(tag_value).ok_or_else(|| {
        error_at(
            path,
            format!("tag field {} must be a scalar value", tag_field),
        )
    })?;

    let variants = tagged_union_variants(schema).ok_or_else(|| {
        error_at(path, "tagged union schema is missing variants".to_string())
    })?;

    let Some(variant_schema) = variants.get(&tag_key) else {
        return Err(error_at(
            path,
            format!("unknown tagged union variant {}", tag_key),
        ));
    };

    let variant_schema = *variant_schema;
    let variant_payload = if should_strip_tag_field(variant_schema, tag_field) {
        let mut filtered = BTreeMap::new();
        for (key, item) in object {
            if key != tag_field {
                filtered.insert(key.clone(), item.clone());
            }
        }
        JsonValue::Object(filtered)
    } else {
        value.clone()
    };

    validate_schema(context, variant_schema, &variant_payload, path, ref_stack)
}

fn should_strip_tag_field(schema: &JsonValue, tag_field: &str) -> bool {
    if !matches!(schema_kind(schema).as_deref(), Some("record")) {
        return false;
    }

    let Ok((fields, _)) = record_fields(schema) else {
        return false;
    };

    !fields.contains_key(tag_field)
}

fn validate_numeric_bounds(schema: &JsonValue, value: f64, path: &[PathSegment]) -> Result<(), String> {
    if let Some(minimum) = schema_number(schema, "minimum") {
        if value < minimum {
            return Err(error_at(
                path,
                format!("value {} is below minimum {}", value, minimum),
            ));
        }
    }

    if let Some(maximum) = schema_number(schema, "maximum") {
        if value > maximum {
            return Err(error_at(
                path,
                format!("value {} is above maximum {}", value, maximum),
            ));
        }
    }

    if let Some(exclusive_minimum) = schema_number(schema, "exclusiveMinimum") {
        if value <= exclusive_minimum {
            return Err(error_at(
                path,
                format!(
                    "value {} is not greater than exclusiveMinimum {}",
                    value, exclusive_minimum
                ),
            ));
        }
    }

    if let Some(exclusive_maximum) = schema_number(schema, "exclusiveMaximum") {
        if value >= exclusive_maximum {
            return Err(error_at(
                path,
                format!(
                    "value {} is not less than exclusiveMaximum {}",
                    value, exclusive_maximum
                ),
            ));
        }
    }

    if let Some(multiple_of) = schema_number(schema, "multipleOf") {
        if multiple_of != 0.0 {
            let ratio = value / multiple_of;
            if (ratio - ratio.round()).abs() > 1e-9 {
                return Err(error_at(
                    path,
                    format!("value {} is not a multiple of {}", value, multiple_of),
                ));
            }
        }
    }

    Ok(())
}

fn record_fields<'a>(
    schema: &'a JsonValue,
) -> Result<(BTreeMap<String, RecordField<'a>>, bool), String> {
    let closed = object_bool(schema, "closed")
        .or_else(|| object_bool(schema, "open").map(|value| !value))
        .or_else(|| object_bool(schema, "additionalProperties").map(|value| !value))
        .unwrap_or(true);

    let mut fields = BTreeMap::new();
    let required_names = object_array(schema, "required")
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_string)
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if let Some(field_values) = object_field(schema, "fields").and_then(JsonValue::as_object) {
        for (name, field_value) in field_values {
            let schema = field_schema(field_value)
                .ok_or_else(|| format!("record field {} does not contain a schema", name))?;
            let required = object_bool(field_value, "required")
                .or_else(|| {
                    if required_names.iter().any(|entry| entry == name) {
                        Some(true)
                    } else {
                        None
                    }
                })
                .unwrap_or(true);
            fields.insert(
                name.clone(),
                RecordField {
                    schema,
                    required,
                },
            );
        }
    } else if let Some(field_values) = object_array(schema, "fields") {
        for field_value in field_values {
            let (name, schema, required) = parse_record_field(field_value, &required_names)?;
            fields.insert(name, RecordField { schema, required });
        }
    } else if let Some(field_values) = object_field(schema, "properties").and_then(JsonValue::as_object)
    {
        for (name, schema_value) in field_values {
            let schema = field_schema(schema_value).ok_or_else(|| {
                format!("record field {} does not contain a schema", name)
            })?;
            let required = object_bool(schema_value, "required")
                .or_else(|| {
                    if required_names.iter().any(|entry| entry == name) {
                        Some(true)
                    } else {
                        None
                    }
                })
                .unwrap_or(true);
            fields.insert(name.clone(), RecordField { schema, required });
        }
    }

    Ok((fields, closed))
}

struct RecordField<'a> {
    schema: &'a JsonValue,
    required: bool,
}

fn parse_record_field<'a>(
    field_value: &'a JsonValue,
    required_names: &[String],
) -> Result<(String, &'a JsonValue, bool), String> {
    let object = field_value
        .as_object()
        .ok_or_else(|| "record field definition must be an object".to_string())?;

    let name = object_string(field_value, "name")
        .or_else(|| object_string(field_value, "key"))
        .or_else(|| object_string(field_value, "field"))
        .ok_or_else(|| "record field definition is missing a name".to_string())?;

    let schema = field_schema(field_value)
        .ok_or_else(|| format!("record field {} does not contain a schema", name))?;

    let required = object_bool(field_value, "required")
        .or_else(|| {
            if required_names.iter().any(|entry| entry == name) {
                Some(true)
            } else {
                None
            }
        })
        .unwrap_or(true);

    if object.get("schema").is_none() && !looks_like_schema(field_value) {
        return Err(format!("record field {} does not contain a schema", name));
    }

    Ok((name.to_string(), schema, required))
}

fn field_schema<'a>(value: &'a JsonValue) -> Option<&'a JsonValue> {
    if let Some(schema) = object_field(value, "schema") {
        Some(schema)
    } else if looks_like_schema(value) {
        Some(value)
    } else {
        None
    }
}

fn has_record_shape(schema: &JsonValue) -> bool {
    object_field(schema, "fields").is_some()
        || object_field(schema, "properties").is_some()
        || object_field(schema, "closed").is_some()
        || object_field(schema, "open").is_some()
        || object_field(schema, "additionalProperties").is_some()
}

fn looks_like_schema(value: &JsonValue) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("kind")
        || object.contains_key("type")
        || object.contains_key("ref")
        || object.contains_key("schema")
        || object.contains_key("fields")
        || object.contains_key("properties")
        || object.contains_key("items")
        || object.contains_key("elem")
        || object.contains_key("item")
        || object.contains_key("element")
        || object.contains_key("variants")
        || object.contains_key("values")
        || object.contains_key("options")
        || object.contains_key("literal")
        || object.contains_key("const")
        || object.contains_key("pattern")
        || object.contains_key("patterns")
        || object.contains_key("format")
        || object.contains_key("minimum")
        || object.contains_key("maximum")
        || object.contains_key("exclusiveMinimum")
        || object.contains_key("exclusiveMaximum")
        || object.contains_key("multipleOf")
}

fn schema_kind(schema: &JsonValue) -> Option<String> {
    let object = schema.as_object()?;
    object
        .get("kind")
        .or_else(|| object.get("type"))
        .and_then(JsonValue::as_string)
        .map(|value| value.to_ascii_lowercase())
        .or_else(|| infer_kind(schema))
}

fn infer_kind(schema: &JsonValue) -> Option<String> {
    let object = schema.as_object()?;
    if object.contains_key("fields")
        || object.contains_key("properties")
        || object.contains_key("closed")
        || object.contains_key("open")
        || object.contains_key("additionalProperties")
    {
        return Some("record".to_string());
    }
    if object.contains_key("items") || object.contains_key("item") || object.contains_key("element")
        || object.contains_key("elem")
    {
        return Some("list".to_string());
    }
    if object.contains_key("variants") {
        if object.contains_key("tag") || object.contains_key("tagField") || object.contains_key("discriminator")
        {
            return Some("taggedunion".to_string());
        }
        return Some("union".to_string());
    }
    if object.contains_key("options") {
        return Some("union".to_string());
    }
    if object.contains_key("values") {
        return Some("enum".to_string());
    }
    if object.contains_key("literal") || object.contains_key("const") {
        return Some("literal".to_string());
    }
    if object.contains_key("valueSchema")
        || object.contains_key("keyPattern")
        || object.contains_key("keyRegex")
    {
        return Some("map".to_string());
    }
    if object.contains_key("pattern")
        || object.contains_key("patterns")
        || object.contains_key("format")
        || object.contains_key("minLength")
        || object.contains_key("maxLength")
    {
        return Some("string".to_string());
    }
    if object.contains_key("minimum")
        || object.contains_key("maximum")
        || object.contains_key("exclusiveMinimum")
        || object.contains_key("exclusiveMaximum")
        || object.contains_key("multipleOf")
    {
        return Some("number".to_string());
    }
    if object.contains_key("ref") {
        return Some("ref".to_string());
    }
    None
}

fn object_field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    value.as_object()?.get(key)
}

fn object_field_mut<'a>(value: &'a mut JsonValue, key: &str) -> Option<&'a mut JsonValue> {
    value.as_object_mut()?.get_mut(key)
}

fn object_string<'a>(value: &'a JsonValue, key: &str) -> Option<&'a str> {
    object_field(value, key)?.as_string()
}

fn object_bool(value: &JsonValue, key: &str) -> Option<bool> {
    object_field(value, key)?.as_bool()
}

fn object_number<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonNumber> {
    object_field(value, key)?.as_number()
}

fn object_array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    object_field(value, key)?.as_array()
}

fn schema_number(schema: &JsonValue, key: &str) -> Option<f64> {
    match object_field(schema, key)? {
        JsonValue::Number(number) => Some(number.float_value),
        JsonValue::String(value) => value.parse::<f64>().ok(),
        _ => None,
    }
}

fn schema_patterns(schema: &JsonValue) -> Option<Vec<String>> {
    let mut patterns = Vec::new();
    if let Some(pattern) = object_string(schema, "pattern") {
        patterns.push(pattern.to_string());
    }
    if let Some(array) = object_array(schema, "patterns") {
        for item in array {
            if let Some(pattern) = item.as_string() {
                patterns.push(pattern.to_string());
            }
        }
    }
    if let Some(format) = object_string(schema, "format") {
        if let Some(pattern) = known_format_pattern(format) {
            patterns.push(pattern.to_string());
        }
    }
    if patterns.is_empty() {
        None
    } else {
        Some(patterns)
    }
}

fn known_format_pattern(format: &str) -> Option<&'static str> {
    match format.to_ascii_lowercase().as_str() {
        "identifier" | "name" | "slug" => {
            Some("^[A-Za-z0-9][A-Za-z0-9._-]*$")
        }
        "timestamp" | "utc-timestamp" | "iso8601-utc" | "rfc3339-utc" => Some(
            "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$",
        ),
        _ => None,
    }
}

fn schema_enum_values(schema: &JsonValue) -> Option<Vec<JsonValue>> {
    if let Some(array) = object_array(schema, "values") {
        return Some(array.iter().cloned().collect());
    }
    if let Some(array) = object_array(schema, "variants") {
        return Some(array.iter().cloned().collect());
    }
    if let Some(array) = object_array(schema, "options") {
        return Some(array.iter().cloned().collect());
    }
    if let Some(object) = object_field(schema, "values")
        .and_then(JsonValue::as_object)
        .or_else(|| object_field(schema, "variants").and_then(JsonValue::as_object))
        .or_else(|| object_field(schema, "options").and_then(JsonValue::as_object))
    {
        let mut values = Vec::new();
        for (key, value) in object {
            match value {
                JsonValue::Null
                | JsonValue::Bool(_)
                | JsonValue::String(_)
                | JsonValue::Number(_) => values.push(value.clone()),
                JsonValue::Array(_) | JsonValue::Object(_) => {
                    values.push(JsonValue::String(key.clone()))
                }
            }
        }
        return Some(values);
    }
    None
}

fn schema_variants(schema: &JsonValue) -> Option<Vec<&JsonValue>> {
    if let Some(object) = object_field(schema, "variants").and_then(JsonValue::as_object) {
        return Some(object.values().collect());
    }
    if let Some(array) = object_array(schema, "variants") {
        return Some(array.iter().collect());
    }
    if let Some(object) = object_field(schema, "options").and_then(JsonValue::as_object) {
        return Some(object.values().collect());
    }
    if let Some(array) = object_array(schema, "options") {
        return Some(array.iter().collect());
    }
    if let Some(array) = object_array(schema, "oneOf") {
        return Some(array.iter().collect());
    }
    if let Some(array) = object_array(schema, "anyOf") {
        return Some(array.iter().collect());
    }
    None
}

fn tagged_union_variants(schema: &JsonValue) -> Option<BTreeMap<String, &JsonValue>> {
    let mut variants = BTreeMap::new();
    if let Some(object) = object_field(schema, "variants").and_then(JsonValue::as_object) {
        for (tag, variant) in object {
            if let Some(schema) = tagged_union_variant_schema(variant) {
                variants.insert(tag.clone(), schema);
            } else {
                variants.insert(tag.clone(), variant);
            }
        }
        return Some(variants);
    }

    if let Some(array) = object_array(schema, "variants") {
        for variant in array {
            let tag = tagged_union_variant_tag(variant)?;
            let schema = tagged_union_variant_schema(variant).unwrap_or(variant);
            variants.insert(tag, schema);
        }
        return Some(variants);
    }

    None
}

fn tagged_union_variant_tag(value: &JsonValue) -> Option<String> {
    if let Some(tag) = object_string(value, "tag") {
        return Some(tag.to_string());
    }
    if let Some(tag) = object_string(value, "value") {
        return Some(tag.to_string());
    }
    if let Some(tag) = object_string(value, "name") {
        return Some(tag.to_string());
    }
    if let Some(tag) = object_string(value, "key") {
        return Some(tag.to_string());
    }
    if let Some(tag) = object_field(value, "literal") {
        return scalar_key(tag);
    }
    None
}

fn tagged_union_variant_schema(value: &JsonValue) -> Option<&JsonValue> {
    object_field(value, "schema")
        .or_else(|| object_field(value, "valueSchema"))
        .or_else(|| object_field(value, "payload"))
        .or_else(|| object_field(value, "content"))
}

fn scalar_key(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(value) => Some(value.clone()),
        JsonValue::Bool(value) => Some(value.to_string()),
        JsonValue::Null => Some("null".to_string()),
        JsonValue::Number(number) if number.integer => {
            if let Some(value) = number.int_value {
                Some(value.to_string())
            } else {
                Some(number.raw.clone())
            }
        }
        JsonValue::Number(number) => Some(number.raw.clone()),
        _ => None,
    }
}

fn json_equal(left: &JsonValue, right: &JsonValue) -> bool {
    match (left, right) {
        (JsonValue::Null, JsonValue::Null) => true,
        (JsonValue::Bool(a), JsonValue::Bool(b)) => a == b,
        (JsonValue::String(a), JsonValue::String(b)) => a == b,
        (JsonValue::Number(a), JsonValue::Number(b)) => {
            if let (Some(ai), Some(bi)) = (a.int_value, b.int_value) {
                ai == bi
            } else {
                a.float_value == b.float_value
            }
        }
        (JsonValue::Array(a), JsonValue::Array(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|(left, right)| json_equal(left, right))
        }
        (JsonValue::Object(a), JsonValue::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get(key)
                        .map(|other| json_equal(value, other))
                        .unwrap_or(false)
                })
        }
        _ => false,
    }
}

fn json_preview(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::String(value) => format!("{:?}", value),
        JsonValue::Number(number) => number.raw.clone(),
        JsonValue::Array(values) => format!("[{} items]", values.len()),
        JsonValue::Object(values) => format!("{{{} keys}}", values.len()),
    }
}

fn resolve_json_path<'a>(value: &'a JsonValue, path_expr: &str) -> Option<&'a JsonValue> {
    let path_expr = path_expr.trim();
    if path_expr.is_empty() || path_expr == "." {
        return Some(value);
    }

    let path_expr = path_expr.strip_prefix('.').unwrap_or(path_expr);
    if path_expr.is_empty() {
        return Some(value);
    }

    let mut current = value;
    for segment in path_expr.split('.') {
        if segment.is_empty() {
            continue;
        }
        current = match current {
            JsonValue::Object(map) => map.get(segment)?,
            JsonValue::Array(items) => {
                let index = segment.parse::<usize>().ok()?;
                items.get(index)?
            }
            _ => return None,
        };
    }

    Some(current)
}

fn json_value_to_number_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Number(number) => Some(number.raw.clone()),
        JsonValue::String(text) => {
            if let Ok(value) = text.parse::<i128>() {
                Some(value.to_string())
            } else if let Ok(value) = text.parse::<f64>() {
                if value.is_finite() {
                    Some(if value.fract() == 0.0 {
                        format!("{:.0}", value)
                    } else {
                        value.to_string()
                    })
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn render_json_compact(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(number) => number.raw.clone(),
        JsonValue::String(value) => format!("\"{}\"", escape_json_string(value)),
        JsonValue::Array(values) => {
            let rendered = values
                .iter()
                .map(render_json_compact)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{}]", rendered)
        }
        JsonValue::Object(values) => {
            let rendered = values
                .iter()
                .map(|(key, item)| {
                    format!("\"{}\":{}", escape_json_string(key), render_json_compact(item))
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{}}}", rendered)
        }
    }
}

fn escape_json_string(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch < ' ' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn value_type(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::String(_) => "string",
        JsonValue::Number(number) if number.integer => "integer",
        JsonValue::Number(_) => "number",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

struct SimplePattern {
    pieces: Vec<PatternPiece>,
}

#[derive(Clone)]
struct PatternPiece {
    atom: PatternAtom,
    min: usize,
    max: Option<usize>,
}

#[derive(Clone)]
enum PatternAtom {
    Literal(char),
    Any,
    Class(CharClass),
}

#[derive(Clone)]
struct CharClass {
    negated: bool,
    items: Vec<CharClassItem>,
}

#[derive(Clone)]
enum CharClassItem {
    Single(char),
    Range(char, char),
    Digit,
    Word,
    Space,
}

impl SimplePattern {
    fn compile(pattern: &str) -> Result<Self, String> {
        let mut source = pattern.trim();
        if source.starts_with('^') && source.ends_with('$') && source.len() >= 2 {
            source = &source[1..source.len() - 1];
        }

        let mut chars = source.chars().peekable();
        let mut pieces = Vec::new();
        while let Some(ch) = chars.next() {
            let atom = match ch {
                '.' => PatternAtom::Any,
                '\\' => PatternAtom::Literal(parse_regex_escape(&mut chars)?),
                '[' => PatternAtom::Class(parse_char_class(&mut chars)?),
                '*' | '+' | '?' | '{' => {
                    return Err(format!("unexpected quantifier '{}' without atom", ch))
                }
                other => PatternAtom::Literal(other),
            };

            let (min, max) = parse_quantifier(&mut chars)?;
            pieces.push(PatternPiece { atom, min, max });
        }

        Ok(Self { pieces })
    }

    fn matches(&self, value: &str) -> bool {
        let chars: Vec<char> = value.chars().collect();
        matches_from(&self.pieces, 0, &chars, 0)
    }
}

fn matches_from(pieces: &[PatternPiece], index: usize, chars: &[char], position: usize) -> bool {
    if index == pieces.len() {
        return position == chars.len();
    }

    let piece = &pieces[index];
    let remaining = chars.len().saturating_sub(position);
    let maximum = piece.max.unwrap_or(remaining).min(remaining);
    let mut counts = Vec::new();
    for count in piece.min..=maximum {
        counts.push(count);
    }

    for count in counts.into_iter().rev() {
        if matches_piece(piece, chars, position, count)
            && matches_from(pieces, index + 1, chars, position + count)
        {
            return true;
        }
    }

    false
}

fn matches_piece(piece: &PatternPiece, chars: &[char], position: usize, count: usize) -> bool {
    for offset in 0..count {
        let Some(ch) = chars.get(position + offset) else {
            return false;
        };
        if !matches_atom(&piece.atom, *ch) {
            return false;
        }
    }
    true
}

fn matches_atom(atom: &PatternAtom, ch: char) -> bool {
    match atom {
        PatternAtom::Literal(expected) => *expected == ch,
        PatternAtom::Any => true,
        PatternAtom::Class(class) => class.matches(ch),
    }
}

fn parse_quantifier(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<(usize, Option<usize>), String> {
    match chars.peek().copied() {
        Some('*') => {
            let _ = chars.next();
            Ok((0, None))
        }
        Some('+') => {
            let _ = chars.next();
            Ok((1, None))
        }
        Some('?') => {
            let _ = chars.next();
            Ok((0, Some(1)))
        }
        Some('{') => {
            let _ = chars.next();
            let minimum = parse_number_literal(chars)?;
            let maximum = match chars.peek().copied() {
                Some(',') => {
                    let _ = chars.next();
                    if matches!(chars.peek(), Some('}')) {
                        None
                    } else {
                        Some(parse_number_literal(chars)?)
                    }
                }
                _ => Some(minimum),
            };
            if chars.next() != Some('}') {
                return Err("unterminated quantifier".to_string());
            }
            if let Some(maximum) = maximum {
                if maximum < minimum {
                    return Err("quantifier maximum is smaller than minimum".to_string());
                }
            }
            Ok((minimum, maximum))
        }
        _ => Ok((1, Some(1))),
    }
}

fn parse_number_literal(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<usize, String> {
    let mut digits = String::new();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            let _ = chars.next();
        } else {
            break;
        }
    }
    if digits.is_empty() {
        return Err("expected number in quantifier".to_string());
    }
    digits
        .parse::<usize>()
        .map_err(|err| format!("invalid quantifier number: {}", err))
}

fn parse_char_class(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<CharClass, String> {
    let mut negated = false;
    if chars.peek() == Some(&'^') {
        negated = true;
        let _ = chars.next();
    }

    let mut items = Vec::new();
    let mut first = true;
    while let Some(ch) = chars.next() {
        if ch == ']' && !first {
            return Ok(CharClass { negated, items });
        }
        first = false;

        let item = if ch == '\\' {
            parse_char_class_escape(chars)?
        } else {
            CharClassItem::Single(ch)
        };

        if let Some('-') = chars.peek().copied() {
            let mut clone = chars.clone();
            let _ = clone.next();
            if let Some(']') = clone.peek().copied() {
                items.push(item);
                items.push(CharClassItem::Single('-'));
                let _ = chars.next();
                continue;
            }

            let _ = chars.next();
            let end = match chars.next() {
                Some(']') | None => return Err("unterminated character class range".to_string()),
                Some('\\') => parse_char_class_escape(chars)?,
                Some(other) => CharClassItem::Single(other),
            };

            let (start_char, end_char) = match (item, end) {
                (CharClassItem::Single(start), CharClassItem::Single(end)) => (start, end),
                _ => {
                    return Err("character class ranges only support literal endpoints".to_string())
                }
            };

            if start_char > end_char {
                return Err("character class range is reversed".to_string());
            }
            items.push(CharClassItem::Range(start_char, end_char));
        } else {
            items.push(item);
        }
    }

    Err("unterminated character class".to_string())
}

fn parse_char_class_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<CharClassItem, String> {
    match chars.next() {
        Some('d') => Ok(CharClassItem::Digit),
        Some('w') => Ok(CharClassItem::Word),
        Some('s') => Ok(CharClassItem::Space),
        Some('n') => Ok(CharClassItem::Single('\n')),
        Some('r') => Ok(CharClassItem::Single('\r')),
        Some('t') => Ok(CharClassItem::Single('\t')),
        Some(other) => Ok(CharClassItem::Single(other)),
        None => Err("unterminated escape in character class".to_string()),
    }
}

fn parse_regex_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<char, String> {
    match chars.next() {
        Some('d') => Err("regex shorthand \\d must be used inside a character class".to_string()),
        Some('w') => Err("regex shorthand \\w must be used inside a character class".to_string()),
        Some('s') => Err("regex shorthand \\s must be used inside a character class".to_string()),
        Some('n') => Ok('\n'),
        Some('r') => Ok('\r'),
        Some('t') => Ok('\t'),
        Some(other) => Ok(other),
        None => Err("unterminated regex escape".to_string()),
    }
}

impl CharClass {
    fn matches(&self, ch: char) -> bool {
        let matched = self.items.iter().any(|item| match item {
            CharClassItem::Single(expected) => *expected == ch,
            CharClassItem::Range(start, end) => *start <= ch && ch <= *end,
            CharClassItem::Digit => ch.is_ascii_digit(),
            CharClassItem::Word => ch.is_ascii_alphanumeric() || ch == '_',
            CharClassItem::Space => ch.is_whitespace(),
        });

        if self.negated {
            !matched
        } else {
            matched
        }
    }
}
