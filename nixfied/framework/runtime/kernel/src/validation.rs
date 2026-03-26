use super::*;

pub(crate) fn validate_input_command(
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

pub(crate) fn validate_scalar_command(spec_path: &str, value: &str) -> Result<(), String> {
    let spec = load_scalar_spec(spec_path)?;
    validate_scalar_value(&spec, value, "scalar")?;
    println!("OK: validate-scalar");
    Ok(())
}

pub(crate) fn validate_exit_command(plan_path: &str, exit_code_text: &str) -> Result<(), String> {
    let plan = load_command_runtime_plan(plan_path)?;
    let exit_code = parse_i32_text(exit_code_text, "exit code")?;
    if exit_code == 0 || plan.failure_codes.contains(&exit_code) {
        println!("OK: validate-exit");
        Ok(())
    } else {
        Err(format!("undeclared exit code code={}", exit_code))
    }
}

pub(crate) fn load_command_runtime_plan(path: &str) -> Result<CommandRuntimePlan, String> {
    let text = read_text(path)?;
    let value = parse_json(&text)
        .map_err(|err| format!("runtime plan {} is not valid JSON: {}", path, err))?;
    parse_command_runtime_plan(&value)
}

pub(crate) fn parse_command_runtime_plan(value: &JsonValue) -> Result<CommandRuntimePlan, String> {
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

pub(crate) fn parse_command_arg_spec(value: &JsonValue) -> Result<CommandArgSpec, String> {
    Ok(CommandArgSpec {
        name: required_string_field(value, "name", "arg spec")?.to_string(),
        kind: required_string_field(value, "kind", "arg spec")?.to_string(),
        scalar: parse_scalar_spec(value)?,
        long: object_string(value, "long").unwrap_or("").to_string(),
        short: object_string(value, "short").unwrap_or("").to_string(),
        required: object_bool(value, "required").unwrap_or(false),
    })
}

pub(crate) fn parse_command_env_spec(value: &JsonValue) -> Result<CommandEnvSpec, String> {
    Ok(CommandEnvSpec {
        name: required_string_field(value, "name", "env spec")?.to_string(),
        scalar: parse_scalar_spec(value)?,
        required: object_bool(value, "required").unwrap_or(false),
        default: object_field(value, "default").and_then(json_value_to_plain_string),
        aliases: array_strings(value, "aliases"),
    })
}

pub(crate) fn load_scalar_spec(path: &str) -> Result<ScalarSpec, String> {
    let text = read_text(path)?;
    let value = parse_json(&text)
        .map_err(|err| format!("scalar spec {} is not valid JSON: {}", path, err))?;
    parse_scalar_spec(&value)
}

pub(crate) fn parse_scalar_spec(value: &JsonValue) -> Result<ScalarSpec, String> {
    Ok(ScalarSpec {
        type_name: object_string(value, "type").unwrap_or("string").to_string(),
        values: array_strings(value, "values"),
        min: object_field(value, "min").and_then(json_value_to_i64),
        max: object_field(value, "max").and_then(json_value_to_i64),
    })
}

pub(crate) fn validate_input_env(
    plan: &CommandRuntimePlan,
) -> Result<Vec<(String, String)>, String> {
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

pub(crate) fn validate_input_args(
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
            return Err(format!(
                "missing required positional arg name={}",
                spec.name
            ));
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

pub(crate) fn resolve_env_spec_value(spec: &CommandEnvSpec) -> Result<Option<String>, String> {
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

    Ok(canonical_non_empty
        .cloned()
        .or_else(|| first_alias.map(|(_, value)| value.clone()))
        .or_else(|| spec.default.clone()))
}

pub(crate) fn validate_scalar_value(
    spec: &ScalarSpec,
    value: &str,
    label: &str,
) -> Result<(), String> {
    match spec.type_name.as_str() {
        "string" => {}
        "bool" => match value {
            "1" | "0" | "true" | "false" | "TRUE" | "FALSE" | "yes" | "YES" | "no" | "NO"
            | "on" | "ON" => {}
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
                return Err(format!(
                    "{} must be an absolute path (got '{}')",
                    label, value
                ));
            }
        }
        "pathRel" => {
            if value.is_empty() || value.starts_with('/') {
                return Err(format!(
                    "{} must be a relative path (got '{}')",
                    label, value
                ));
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

pub(crate) fn validate_numeric_range(
    value: i64,
    spec: &ScalarSpec,
    label: &str,
) -> Result<(), String> {
    if let Some(minimum) = spec.min {
        if value < minimum {
            return Err(format!(
                "{} must be >= {} (got '{}')",
                label, minimum, value
            ));
        }
    }
    if let Some(maximum) = spec.max {
        if value > maximum {
            return Err(format!(
                "{} must be <= {} (got '{}')",
                label, maximum, value
            ));
        }
    }
    Ok(())
}

pub(crate) fn export_arg_name(name: &str) -> String {
    format!("NIXFIED_ARG_{}", sanitize_name(name))
}

pub(crate) fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '.' | ':' | '/' | '-' => '_',
            other => other.to_ascii_uppercase(),
        })
        .collect()
}

pub(crate) fn is_runtime_primitive_env(name: &str) -> bool {
    matches!(name, "LOG_LEVEL" | "OUTPUT_MODE")
}

pub(crate) fn validate_file_command(
    command: &str,
    bundle_path: &str,
    contract_ref: &str,
    payload_path: &str,
) -> Result<(), String> {
    validate_json_file_against_contract(bundle_path, contract_ref, payload_path)?;
    println!("OK: {} contract={}", command, contract_ref);
    Ok(())
}

pub(crate) fn validate_json_file_against_contract(
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

pub(crate) fn validate_json_value_against_contract(
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
