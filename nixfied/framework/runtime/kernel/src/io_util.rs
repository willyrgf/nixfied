use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

pub(crate) fn read_text(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|err| format!("failed to read stdin: {}", err))?;
        return Ok(buffer);
    }

    fs::read_to_string(path).map_err(|err| format!("failed to read {}: {}", path, err))
}

pub(crate) fn write_text_atomic(path: &str, contents: &str) -> Result<(), String> {
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

pub(crate) fn write_shell_exports(path: &str, values: &[(String, String)]) -> Result<(), String> {
    let rendered = render_shell_exports(values);
    write_text_atomic(path, &rendered)
}

pub(crate) fn write_lines_atomic(path: &str, lines: &[String]) -> Result<(), String> {
    write_text_atomic(path, &render_lines(lines))
}

pub(crate) fn render_shell_exports(values: &[(String, String)]) -> String {
    let mut rendered = String::new();
    for (key, value) in values {
        rendered.push_str("export ");
        rendered.push_str(key);
        rendered.push('=');
        rendered.push_str(&shell_quote(value));
        rendered.push('\n');
    }
    rendered
}

pub(crate) fn render_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(crate) fn append_line(path: &str, line: &str) -> Result<(), String> {
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
