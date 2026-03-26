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
    definition_maps: Vec<&'a Map<String, JsonValue>>,
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
                if let Some(definitions) =
                    validation.get("definitions").and_then(JsonValue::as_object)
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
        Some(other) => Err(error_at(
            path,
            format!("unsupported schema kind '{}'", other),
        )),
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
        other => Err(error_at(
            path,
            format!("expected bool, found {}", value_type(other)),
        )),
    }
}

fn validate_null(value: &JsonValue, path: &[PathSegment]) -> Result<(), String> {
    match value {
        JsonValue::Null => Ok(()),
        other => Err(error_at(
            path,
            format!("expected null, found {}", value_type(other)),
        )),
    }
}

fn validate_string(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let string = match value {
        JsonValue::String(value) => value,
        other => {
            return Err(error_at(
                path,
                format!("expected string, found {}", value_type(other)),
            ))
        }
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
        JsonValue::Number(n) if n.is_i64() || n.is_u64() => n,
        JsonValue::Number(other) => {
            return Err(error_at(
                path,
                format!("expected integer, found number {}", other),
            ))
        }
        other => {
            return Err(error_at(
                path,
                format!("expected integer, found {}", value_type(other)),
            ))
        }
    };

    validate_numeric_bounds(schema, number.as_f64().unwrap_or(0.0), path)?;
    Ok(())
}

fn validate_number(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let number = match value {
        JsonValue::Number(n) => n,
        other => {
            return Err(error_at(
                path,
                format!("expected number, found {}", value_type(other)),
            ))
        }
    };

    validate_numeric_bounds(schema, number.as_f64().unwrap_or(0.0), path)?;
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
        return Err(error_at(
            path,
            "literal schema is missing a value".to_string(),
        ));
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
        other => {
            return Err(error_at(
                path,
                format!("expected array, found {}", value_type(other)),
            ))
        }
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
            if let Some(rest_schema) =
                object_field(schema, "rest").or_else(|| object_field(schema, "additionalItems"))
            {
                for (index, item) in items.iter().enumerate().skip(tuple_items.len()) {
                    path.push(PathSegment::Index(index));
                    let result = validate_schema(context, rest_schema, item, path, ref_stack);
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
        other => {
            return Err(error_at(
                path,
                format!("expected object, found {}", value_type(other)),
            ))
        }
    };

    let value_schema = object_field(schema, "value")
        .or_else(|| object_field(schema, "valueSchema"))
        .or_else(|| object_field(schema, "schema"));

    let key_pattern = object_string(schema, "keyPattern")
        .or_else(|| object_string(schema, "pattern"))
        .or_else(|| object_string(schema, "keyRegex"));

    let compiled_key_pattern =
        if let Some(pattern) = key_pattern {
            Some(SimplePattern::compile(pattern).map_err(|err| {
                error_at(path, format!("invalid key pattern {}: {}", pattern, err))
            })?)
        } else {
            None
        };

    for (key, item) in object {
        if let Some(pattern) = &compiled_key_pattern {
            if !pattern.matches(key) {
                return Err(error_at(
                    path,
                    format!("key {} does not match key pattern", key),
                ));
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
        other => {
            return Err(error_at(
                path,
                format!("expected object, found {}", value_type(other)),
            ))
        }
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
    let variants = schema_variants(schema)
        .ok_or_else(|| error_at(path, "union schema is missing variants".to_string()))?;

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
        other => {
            return Err(error_at(
                path,
                format!("expected object, found {}", value_type(other)),
            ))
        }
    };

    let tag_field = object_string(schema, "tag")
        .or_else(|| object_string(schema, "tagField"))
        .or_else(|| object_string(schema, "discriminator"))
        .unwrap_or("tag");
    let tag_value = object
        .get(tag_field)
        .ok_or_else(|| error_at(path, format!("missing tagged union field {}", tag_field)))?;

    let tag_key = scalar_key(tag_value).ok_or_else(|| {
        error_at(
            path,
            format!("tag field {} must be a scalar value", tag_field),
        )
    })?;

    let variants = tagged_union_variants(schema)
        .ok_or_else(|| error_at(path, "tagged union schema is missing variants".to_string()))?;

    let Some(variant_schema) = variants.get(&tag_key) else {
        return Err(error_at(
            path,
            format!("unknown tagged union variant {}", tag_key),
        ));
    };

    let variant_schema = *variant_schema;
    let variant_payload = if should_strip_tag_field(variant_schema, tag_field) {
        let mut filtered = Map::new();
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

fn validate_numeric_bounds(
    schema: &JsonValue,
    value: f64,
    path: &[PathSegment],
) -> Result<(), String> {
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
                .filter_map(JsonValue::as_str)
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
            fields.insert(name.clone(), RecordField { schema, required });
        }
    } else if let Some(field_values) = object_array(schema, "fields") {
        for field_value in field_values {
            let (name, schema, required) = parse_record_field(field_value, &required_names)?;
            fields.insert(name, RecordField { schema, required });
        }
    } else if let Some(field_values) =
        object_field(schema, "properties").and_then(JsonValue::as_object)
    {
        for (name, schema_value) in field_values {
            let schema = field_schema(schema_value)
                .ok_or_else(|| format!("record field {} does not contain a schema", name))?;
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
        .and_then(JsonValue::as_str)
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
    if object.contains_key("items")
        || object.contains_key("item")
        || object.contains_key("element")
        || object.contains_key("elem")
    {
        return Some("list".to_string());
    }
    if object.contains_key("variants") {
        if object.contains_key("tag")
            || object.contains_key("tagField")
            || object.contains_key("discriminator")
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

fn schema_number(schema: &JsonValue, key: &str) -> Option<f64> {
    match object_field(schema, key)? {
        JsonValue::Number(n) => n.as_f64(),
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
            if let Some(pattern) = item.as_str() {
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
        "identifier" | "name" | "slug" => Some("^[A-Za-z0-9][A-Za-z0-9._-]*$"),
        "timestamp" | "utc-timestamp" | "iso8601-utc" | "rfc3339-utc" => {
            Some("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
        }
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
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i.to_string())
            } else if let Some(u) = n.as_u64() {
                Some(u.to_string())
            } else {
                Some(n.to_string())
            }
        }
        _ => None,
    }
}

fn json_equal(left: &JsonValue, right: &JsonValue) -> bool {
    left == right
}

fn json_preview(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::String(value) => format!("{:?}", value),
        JsonValue::Number(number) => number.to_string(),
        JsonValue::Array(values) => format!("[{} items]", values.len()),
        JsonValue::Object(values) => format!("{{{} keys}}", values.len()),
    }
}

pub(crate) fn resolve_json_path<'a>(value: &'a JsonValue, path_expr: &str) -> Option<&'a JsonValue> {
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

fn value_type(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::String(_) => "string",
        JsonValue::Number(n) if n.is_i64() || n.is_u64() => "integer",
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
