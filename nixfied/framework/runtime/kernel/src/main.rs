use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

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
        "validate-json" => {
            let input_path = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            validate_json_command(&input_path)
        }
        "json-length" => {
            let input_path = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            json_length_command(&input_path)
        }
        "query-json" => {
            let input_path = args.next().ok_or_else(usage)?;
            let path_expr = args.next().ok_or_else(usage)?;
            let mut raw = false;
            let mut empty_ok = false;
            let mut tonumber = false;
            for flag in args {
                match flag.as_str() {
                    "--raw" => raw = true,
                    "--compact" => raw = false,
                    "--empty-ok" | "--empty" => empty_ok = true,
                    "--tonumber" => tonumber = true,
                    other => {
                        return Err(format!(
                            "unknown query-json flag: {}\n{}",
                            other,
                            usage()
                        ))
                    }
                }
            }
            query_json_command(&input_path, &path_expr, raw, empty_ok, tonumber)
        }
        "validate-payload" | "validate-input" | "validate-artifact" | "validate-scalar" => {
            let bundle_path = args.next().ok_or_else(usage)?;
            let contract_ref = args.next().ok_or_else(usage)?;
            let payload_path = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            validate_file_command(&command, &bundle_path, &contract_ref, &payload_path)
        }
        other => Err(format!("unknown command: {}\n{}", other, usage())),
    }
}

fn usage() -> String {
    [
        "usage: nixfied-kernel <command> [args...]",
        "",
        "commands:",
        "  validate-json <json-file>",
        "  json-length <json-file>",
        "  query-json <json-file> <path> [--raw|--compact] [--empty-ok] [--tonumber]",
        "  validate-payload <bundle-file> <contract-ref> <payload-file>",
        "  validate-input   <bundle-file> <contract-ref> <payload-file>",
        "  validate-artifact <bundle-file> <contract-ref> <payload-file>",
        "  validate-scalar  <bundle-file> <contract-ref> <payload-file>",
        "  help",
    ]
    .join("\n")
}

fn print_help() {
    println!("{}", usage());
}

fn validate_file_command(
    command: &str,
    bundle_path: &str,
    contract_ref: &str,
    payload_path: &str,
) -> Result<(), String> {
    let bundle_text = read_text(bundle_path)?;
    let payload_text = read_text(payload_path)?;

    let bundle = parse_json(&bundle_text).map_err(|err| {
        format!("bundle {} is not valid JSON: {}", bundle_path, err)
    })?;
    let payload = parse_json(&payload_text).map_err(|err| {
        format!("payload {} is not valid JSON: {}", payload_path, err)
    })?;

    let context = ValidationContext::new(&bundle);
    let schema = context.resolve_contract_ref(contract_ref).map_err(|err| {
        format!("contract {} could not be resolved: {}", contract_ref, err)
    })?;

    let mut path = Vec::new();
    let mut ref_stack = Vec::new();
    validate_schema(&context, schema, &payload, &mut path, &mut ref_stack).map_err(|err| {
        format!("contract {} failed validation: {}", contract_ref, err)
    })?;

    println!("OK: {} contract={}", command, contract_ref);
    Ok(())
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

    fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(value) => Some(value.as_slice()),
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
