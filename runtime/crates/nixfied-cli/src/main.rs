use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use nixfied_model::{Model, Validate};
use serde_json::{Value, json};

const DEFAULT_NIXFIED_URL: &str = "github:willyrgf/nixfied";

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if let Err(error) = run(args) {
        eprintln!("{}", error.message);
        std::process::exit(error.exit_code);
    }
}

fn run(args: Vec<OsString>) -> Result<(), CliError> {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        print_usage();
        return Ok(());
    };
    match command {
        "model" | "schema" | "docs" | "capabilities" => {
            if args[1..].iter().any(|arg| arg == "-h" || arg == "--help") {
                print_view_usage(command);
                return Ok(());
            }
            let options = ViewOptions::parse(&args[1..])?;
            let output = render_view(command, &options)?;
            print!("{output}");
            Ok(())
        }
        "install" => {
            if args[1..].iter().any(|arg| arg == "-h" || arg == "--help") {
                print_install_usage();
                return Ok(());
            }
            let options = InstallOptions::parse(&args[1..])?;
            let outcome = install(&options)?;
            print_install_outcome(&outcome);
            Ok(())
        }
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(())
        }
        other => Err(CliError::usage(format!(
            "unsupported nixfied command: {other}"
        ))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewOptions {
    model: PathBuf,
}

impl ViewOptions {
    fn parse(args: &[OsString]) -> Result<Self, CliError> {
        let mut model = None;
        let mut index = 0;
        while index < args.len() {
            let Some(arg) = args[index].to_str() else {
                return Err(CliError::usage("arguments must be valid UTF-8"));
            };
            match arg {
                "--model" => {
                    index += 1;
                    model = Some(take_value(args, index, "--model")?.into());
                }
                other => {
                    return Err(CliError::usage(format!("unknown view argument: {other}")));
                }
            }
            index += 1;
        }
        let Some(model) = model else {
            return Err(CliError::usage("missing --model PATH"));
        };
        Ok(Self { model })
    }
}

/// Framework-owned public surfaces: the generated view/runtime command set every
/// model exposes. A view concern, derived here rather than carried in the model.
const SURFACE_NAMES: &[&str] = &[
    "model",
    "schema",
    "docs",
    "capabilities",
    "check",
    "run",
    "ps",
    "down",
    "clean",
];

/// The lifecycle classes every service declares, in canonical order. The typed
/// `Lifecycle` makes this set closed, so the view states it rather than probing.
const LIFECYCLE_CLASSES: &str = "prepare, start, ready, health, stop, clean";

fn render_view(command: &str, options: &ViewOptions) -> Result<String, CliError> {
    let model = read_model(&options.model)?;
    match command {
        "model" => json_output(&to_json(&model)?),
        "schema" => json_output(&schema_view(&model)),
        "docs" => Ok(docs_view(&model)),
        "capabilities" => json_output(&capabilities_view(&model)),
        other => Err(CliError::usage(format!(
            "unsupported nixfied view command: {other}"
        ))),
    }
}

/// Parse and validate against the same typed contract the runtime admits
/// (`deny_unknown_fields` plus the model's own identity/structure checks), so a
/// model the CLI renders is one the runtime would accept.
fn read_model(path: &Path) -> Result<Model, CliError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| CliError::io(format!("failed to read {}: {error}", path.display())))?;
    let model: Model = serde_json::from_str(&contents)
        .map_err(|error| CliError::usage(format!("failed to parse {}: {error}", path.display())))?;
    model
        .validate()
        .map_err(|error| CliError::usage(format!("invalid model {}: {error}", path.display())))?;
    Ok(model)
}

/// The capabilities view is a projection of the model, derived on demand rather
/// than read from a redundant model section.
fn capabilities_view(model: &Model) -> Value {
    json!({
        "environments": model.environments.keys().collect::<Vec<_>>(),
        "slots": (model.slot_policy.min..=model.slot_policy.max).collect::<Vec<_>>(),
        "services": model.services.keys().collect::<Vec<_>>(),
        "tasks": model.tasks.keys().collect::<Vec<_>>(),
        "workflows": model.workflows.keys().collect::<Vec<_>>(),
        "surfaces": SURFACE_NAMES,
    })
}

/// Identities come from the contract constants, not echoed from the file —
/// `read_model` already proved the file matches them.
fn schema_view(_model: &Model) -> Value {
    json!({
        "schemaVersion": 1,
        "source": "model.json",
        "surfaces": SURFACE_NAMES,
        "modelTypes": {
            "modelVersion": nixfied_model::MODEL_VERSION,
            "runtimeAbi": nixfied_model::runtime_abi(),
            "toolchainId": nixfied_model::TOOLCHAIN_ID,
            "primitives": [
                "ExecSpec",
                "Endpoint",
                "ProbeSpec",
                "Lifecycle",
                "TerminalSemantics",
                "ServiceSpec",
                "TaskSpec",
                "SlotPlacement"
            ]
        }
    })
}

fn docs_view(model: &Model) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "# {}", model.docs.title);
    let _ = writeln!(output);
    let _ = writeln!(output, "{}", model.docs.summary);
    let _ = writeln!(output);
    let _ = writeln!(output, "## Target");
    let _ = writeln!(output);
    let _ = writeln!(output, "- system: {}", model.target.system);
    let _ = writeln!(output, "- runtime ABI: {}", model.runtime_abi);
    let _ = writeln!(output, "- toolchain: {}", model.toolchain_id);
    let _ = writeln!(output);
    let _ = writeln!(output, "## Surfaces");
    let _ = writeln!(output);
    for name in SURFACE_NAMES {
        let _ = writeln!(output, "- {name}");
    }
    let _ = writeln!(output);
    let _ = writeln!(output, "## Services");
    let _ = writeln!(output);
    for name in model.services.keys() {
        let _ = writeln!(output, "- {name}");
    }
    let _ = writeln!(output);
    let _ = writeln!(output, "## Lifecycle");
    let _ = writeln!(output);
    for (name, spec) in &model.services {
        let _ = writeln!(
            output,
            "- {name}: endpoint {}; operations {LIFECYCLE_CLASSES}",
            spec.endpoint.endpoint_id
        );
    }
    let _ = writeln!(output);
    let _ = writeln!(output, "## Tasks");
    let _ = writeln!(output);
    for name in model.tasks.keys() {
        let _ = writeln!(output, "- {name}");
    }
    output
}

fn to_json(model: &Model) -> Result<Value, CliError> {
    serde_json::to_value(model)
        .map_err(|error| CliError::io(format!("failed to render model: {error}")))
}

fn json_output(value: &Value) -> Result<String, CliError> {
    serde_json::to_string_pretty(value)
        .map(|json| format!("{json}\n"))
        .map_err(|error| CliError::io(format!("failed to render JSON: {error}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallOptions {
    root: PathBuf,
    project_id: Option<String>,
    name: Option<String>,
    nixfied_url: String,
}

impl InstallOptions {
    fn parse(args: &[OsString]) -> Result<Self, CliError> {
        let mut root = PathBuf::from(".");
        let mut project_id = None;
        let mut name = None;
        let mut nixfied_url = DEFAULT_NIXFIED_URL.to_string();
        let mut index = 0;
        while index < args.len() {
            let Some(arg) = args[index].to_str() else {
                return Err(CliError::usage("arguments must be valid UTF-8"));
            };
            match arg {
                "--root" => {
                    index += 1;
                    root = take_value(args, index, "--root")?.into();
                }
                "--project-id" => {
                    index += 1;
                    project_id = Some(take_value(args, index, "--project-id")?.to_string());
                }
                "--name" => {
                    index += 1;
                    name = Some(take_value(args, index, "--name")?.to_string());
                }
                "--nixfied-url" => {
                    index += 1;
                    nixfied_url = take_value(args, index, "--nixfied-url")?.to_string();
                }
                other => {
                    return Err(CliError::usage(format!(
                        "unknown install argument: {other}"
                    )));
                }
            }
            index += 1;
        }
        Ok(Self {
            root,
            project_id,
            name,
            nixfied_url,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallOutcome {
    root: PathBuf,
    project_id: String,
    name: String,
    created: Vec<&'static str>,
    skipped: Vec<&'static str>,
}

fn install(options: &InstallOptions) -> Result<InstallOutcome, CliError> {
    let root = options.root.clone();
    let metadata = ProjectMetadata::resolve(&root, options)?;
    validate_project_id(&metadata.project_id)?;
    let flake_path = root.join("flake.nix");
    if flake_path.exists() {
        return Err(CliError::refusal(existing_flake_message(
            &root,
            &metadata,
            &options.nixfied_url,
        )));
    }

    std::fs::create_dir_all(&root).map_err(|error| {
        CliError::io(format!(
            "failed to create target directory {}: {error}",
            root.display()
        ))
    })?;

    let mut created = Vec::new();
    let mut skipped = Vec::new();
    write_new_file(
        &flake_path,
        &flake_template(&options.nixfied_url),
        "flake.nix",
        &mut created,
        &mut skipped,
    )?;
    write_new_file(
        &root.join("nixfied.nix"),
        &nixfied_module_template(&metadata),
        "nixfied.nix",
        &mut created,
        &mut skipped,
    )?;

    Ok(InstallOutcome {
        root,
        project_id: metadata.project_id,
        name: metadata.name,
        created,
        skipped,
    })
}

fn write_new_file(
    path: &Path,
    contents: &str,
    label: &'static str,
    created: &mut Vec<&'static str>,
    skipped: &mut Vec<&'static str>,
) -> Result<(), CliError> {
    if path.exists() {
        skipped.push(label);
        return Ok(());
    }
    std::fs::write(path, contents)
        .map_err(|error| CliError::io(format!("failed to write {}: {error}", path.display())))?;
    created.push(label);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectMetadata {
    project_id: String,
    name: String,
}

impl ProjectMetadata {
    fn resolve(root: &Path, options: &InstallOptions) -> Result<Self, CliError> {
        let inferred = infer_project_name(root)?;
        Ok(Self {
            project_id: options
                .project_id
                .clone()
                .unwrap_or_else(|| inferred.clone()),
            name: options.name.clone().unwrap_or(inferred),
        })
    }
}

fn infer_project_name(root: &Path) -> Result<String, CliError> {
    let path = if root == Path::new(".") {
        std::env::current_dir().map_err(|error| {
            CliError::io(format!("failed to inspect current directory: {error}"))
        })?
    } else {
        root.to_path_buf()
    };
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .ok_or_else(|| CliError::usage("could not infer project name from --root"))
}

fn validate_project_id(project_id: &str) -> Result<(), CliError> {
    let mut chars = project_id.chars();
    let Some(first) = chars.next() else {
        return Err(invalid_project_id(project_id));
    };
    if !first.is_ascii_alphanumeric()
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(invalid_project_id(project_id));
    }
    Ok(())
}

fn invalid_project_id(project_id: &str) -> CliError {
    CliError::usage(format!(
        "invalid project id {project_id:?}; expected [A-Za-z0-9][A-Za-z0-9._-]*"
    ))
}

fn flake_template(nixfied_url: &str) -> String {
    format!(
        r#"{{
  description = "Nixfied project";

  inputs = {{
    nixfied.url = "{}";
  }};

  outputs =
    {{ self, nixfied }}:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
      forAllSystems = f: builtins.listToAttrs (
        map (system: {{
          name = system;
          value = f system;
        }}) systems
      );
    in
    {{
      packages = forAllSystems (system: {{
        default = self.packages.${{system}}.model;
        model = nixfied.lib.${{system}}.compileModel ./nixfied.nix;
      }});
      # `nix run .#run` / `.#check` / `.#test` / `.#ci` — your project's run and
      # verification surface, generated from the model.
      apps = forAllSystems (system: nixfied.lib.${{system}}.projectApps ./nixfied.nix);
    }};
}}
"#,
        nix_escape(nixfied_url)
    )
}

fn nixfied_module_template(metadata: &ProjectMetadata) -> String {
    format!(
        r#"{{ adapters, ... }}:
{{
  # The synthetic adapter is a runnable starter service so `nix build .#model`
  # succeeds out of the box. Replace it with your own service/task declarations
  # or another adapter (e.g. adapters.postgres).
  imports = [ adapters.synthetic ];

  nixfied.project.projectId = "{}";
  nixfied.project.name = "{}";
  nixfied.codebases.main.logicalRoot = ".";

  # `nix run .#test` runs the `test` workflow. This starter wraps the synthetic
  # adapter's `smoke` task (it pings the service). Replace it with your own
  # tasks: a 0-service `fullcheck` (lint/test) or an N-service `e2e`.
  nixfied.workflows.test = {{
    servicesRequired = [ "synthetic" ];
    nodes = {{
      smoke = {{
        taskId = "smoke";
        dependsOn = [ ];
      }};
    }};
  }};
}}
"#,
        nix_escape(&metadata.project_id),
        nix_escape(&metadata.name)
    )
}

fn existing_flake_message(root: &Path, metadata: &ProjectMetadata, nixfied_url: &str) -> String {
    let mut message = String::new();
    let _ = writeln!(
        message,
        "refusing to modify existing flake.nix in {}",
        root.display()
    );
    let _ = writeln!(message, "No files were changed.");
    let _ = writeln!(message);
    let _ = writeln!(message, "Add this input and package wiring manually:");
    let _ = writeln!(message);
    let _ = writeln!(message, "{}", flake_merge_snippet(nixfied_url));
    let _ = writeln!(message, "Then create nixfied.nix if it does not exist:");
    let _ = writeln!(message);
    let _ = write!(message, "{}", nixfied_module_template(metadata));
    message
}

fn flake_merge_snippet(nixfied_url: &str) -> String {
    format!(
        r#"inputs.nixfied.url = "{}";

packages.${{system}}.model = nixfied.lib.${{system}}.compileModel ./nixfied.nix;

# `nix run .#run` / `.#check` / `.#test` / `.#ci` — your project's run and
# verification surface, generated from the model.
apps.${{system}} = nixfied.lib.${{system}}.projectApps ./nixfied.nix;
"#,
        nix_escape(nixfied_url)
    )
}

fn nix_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn take_value<'a>(args: &'a [OsString], index: usize, flag: &str) -> Result<&'a str, CliError> {
    args.get(index)
        .and_then(|arg| arg.to_str())
        .ok_or_else(|| CliError::usage(format!("missing {flag} value")))
}

fn print_install_outcome(outcome: &InstallOutcome) {
    println!("Installed Nixfied scaffold in {}", outcome.root.display());
    println!("projectId: {}", outcome.project_id);
    println!("name: {}", outcome.name);
    if !outcome.created.is_empty() {
        println!("created: {}", outcome.created.join(", "));
    }
    if !outcome.skipped.is_empty() {
        println!("skipped: {}", outcome.skipped.join(", "));
    }
    println!("next: nix build .#model");
}

fn print_usage() {
    println!("nixfied");
    print_view_usage("model");
    print_view_usage("schema");
    print_view_usage("docs");
    print_view_usage("capabilities");
    print_install_usage();
}

fn print_view_usage(command: &str) {
    println!("usage: nixfied {command} --model PATH");
}

fn print_install_usage() {
    println!(
        "usage: nixfied install [--root PATH] [--project-id ID] [--name NAME] [--nixfied-url URL]"
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CliError {
    exit_code: i32,
    message: String,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            exit_code: 2,
            message: message.into(),
        }
    }

    fn refusal(message: impl Into<String>) -> Self {
        Self {
            exit_code: 3,
            message: message.into(),
        }
    }

    fn io(message: impl Into<String>) -> Self {
        Self {
            exit_code: 1,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn installs_fresh_project() {
        let tmp = TempDir::new();
        let root = tmp.path.join("install-proof");
        let outcome = install(&InstallOptions {
            root: root.clone(),
            project_id: Some("install-proof".to_string()),
            name: Some("Install Proof".to_string()),
            nixfied_url: "path:/repo".to_string(),
        })
        .expect("fresh install should succeed");

        assert_eq!(outcome.created, vec!["flake.nix", "nixfied.nix"]);
        assert!(root.join("flake.nix").is_file());
        assert!(root.join("nixfied.nix").is_file());
        let flake = read(&root.join("flake.nix"));
        assert!(flake.contains("nixfied.url = \"path:/repo\""));
        // The scaffold wires the generated run/check/test/ci app surface.
        assert!(flake.contains("projectApps ./nixfied.nix"));
        let module = read(&root.join("nixfied.nix"));
        assert!(module.contains("nixfied.project.projectId = \"install-proof\""));
        // The scaffold ships a runnable service so `nix build .#model` builds,
        // and a `test` workflow so `nix run .#test` works out of the box.
        assert!(module.contains("imports = [ adapters.synthetic ];"));
        assert!(module.contains("nixfied.workflows.test"));
    }

    #[test]
    fn refuses_existing_flake_without_writing_config() {
        let tmp = TempDir::new();
        let root = tmp.path.join("existing-flake");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("flake.nix"), "{}\n").unwrap();

        let error = install(&InstallOptions {
            root: root.clone(),
            project_id: Some("existing-flake".to_string()),
            name: None,
            nixfied_url: DEFAULT_NIXFIED_URL.to_string(),
        })
        .expect_err("existing flake should refuse");

        assert_eq!(error.exit_code, 3);
        assert!(!root.join("nixfied.nix").exists());
        assert_eq!(read(&root.join("flake.nix")), "{}\n");
        assert!(error.message.contains("No files were changed."));
        // The merge instructions must wire the same surface the fresh-flake
        // template does: the model package and the generated apps.
        assert!(error.message.contains("compileModel ./nixfied.nix"));
        assert!(
            error
                .message
                .contains("apps.${system} = nixfied.lib.${system}.projectApps ./nixfied.nix;")
        );
    }

    #[test]
    fn preserves_existing_nixfied_module() {
        let tmp = TempDir::new();
        let root = tmp.path.join("existing-config");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("nixfied.nix"), "{ custom = true; }\n").unwrap();

        let outcome = install(&InstallOptions {
            root: root.clone(),
            project_id: None,
            name: None,
            nixfied_url: DEFAULT_NIXFIED_URL.to_string(),
        })
        .expect("install should create only flake");

        assert_eq!(outcome.created, vec!["flake.nix"]);
        assert_eq!(outcome.skipped, vec!["nixfied.nix"]);
        assert_eq!(outcome.project_id, "existing-config");
        assert_eq!(read(&root.join("nixfied.nix")), "{ custom = true; }\n");
    }

    #[test]
    fn rejects_invalid_project_id() {
        let tmp = TempDir::new();
        let error = install(&InstallOptions {
            root: tmp.path.join("bad"),
            project_id: Some("-bad".to_string()),
            name: None,
            nixfied_url: DEFAULT_NIXFIED_URL.to_string(),
        })
        .expect_err("invalid project id should fail");

        assert_eq!(error.exit_code, 2);
        assert!(error.message.contains("invalid project id"));
    }

    #[test]
    fn parses_install_options() {
        let options = InstallOptions::parse(&[
            "--root".into(),
            "repo".into(),
            "--project-id".into(),
            "proj".into(),
            "--name".into(),
            "Project".into(),
            "--nixfied-url".into(),
            "path:/repo".into(),
        ])
        .expect("args should parse");

        assert_eq!(options.root, PathBuf::from("repo"));
        assert_eq!(options.project_id.as_deref(), Some("proj"));
        assert_eq!(options.name.as_deref(), Some("Project"));
        assert_eq!(options.nixfied_url, "path:/repo");
    }

    #[test]
    fn parses_view_options() {
        let options =
            ViewOptions::parse(&["--model".into(), "model.json".into()]).expect("args parse");

        assert_eq!(options.model, PathBuf::from("model.json"));
    }

    #[test]
    fn renders_model_from_model_json() {
        let tmp = TempDir::new();
        let model_path = tmp.path.join("model.json");
        std::fs::write(&model_path, model_fixture()).unwrap();

        let output = render_view("model", &ViewOptions { model: model_path })
            .expect("model view should render");
        let output: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["project"]["projectId"], "view-test");
        assert!(output["services"].get("synthetic").is_some());
    }

    #[test]
    fn renders_schema_from_model_json() {
        let tmp = TempDir::new();
        let model_path = tmp.path.join("model.json");
        std::fs::write(&model_path, model_fixture()).unwrap();

        let output = render_view("schema", &ViewOptions { model: model_path })
            .expect("schema view should render");
        let output: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["source"], "model.json");
        assert_eq!(
            output["modelTypes"]["runtimeAbi"],
            nixfied_model::runtime_abi()
        );
        assert_eq!(output["surfaces"][1], "schema");
    }

    /// The views enforce the runtime's contract: an unknown field (a stale or
    /// hand-edited model) is rejected instead of rendered.
    #[test]
    fn rejects_model_with_unknown_field() {
        let tmp = TempDir::new();
        let model_path = tmp.path.join("model.json");
        let mut model: serde_json::Value = serde_json::from_str(&model_fixture()).unwrap();
        model["unknownField"] = serde_json::json!(true);
        std::fs::write(&model_path, model.to_string()).unwrap();

        let error = render_view("docs", &ViewOptions { model: model_path })
            .expect_err("unknown field should be rejected");

        assert_eq!(error.exit_code, 2);
        assert!(error.message.contains("failed to parse"));
    }

    /// Identity mismatches fail with the model's own validation error, the same
    /// check the runtime applies at admission.
    #[test]
    fn rejects_model_with_stale_runtime_abi() {
        let tmp = TempDir::new();
        let model_path = tmp.path.join("model.json");
        let mut model: serde_json::Value = serde_json::from_str(&model_fixture()).unwrap();
        model["runtimeAbi"] = serde_json::json!("nixfied-runtime-abi:1-000000000000");
        std::fs::write(&model_path, model.to_string()).unwrap();

        let error = render_view("schema", &ViewOptions { model: model_path })
            .expect_err("stale ABI should be rejected");

        assert_eq!(error.exit_code, 2);
        assert!(error.message.contains("invalid model"));
    }

    #[test]
    fn renders_capabilities_from_model_json() {
        let tmp = TempDir::new();
        let model_path = tmp.path.join("model.json");
        std::fs::write(&model_path, model_fixture()).unwrap();

        let output = render_view("capabilities", &ViewOptions { model: model_path })
            .expect("capabilities view should render");
        let output: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["services"], serde_json::json!(["synthetic"]));
        assert_eq!(output["surfaces"][3], "capabilities");
    }

    #[test]
    fn renders_docs_from_model_json() {
        let tmp = TempDir::new();
        let model_path = tmp.path.join("model.json");
        std::fs::write(&model_path, model_fixture()).unwrap();

        let output =
            render_view("docs", &ViewOptions { model: model_path }).expect("docs view renders");

        assert!(output.contains("# View Test"));
        assert!(output.contains("- schema"));
        assert!(output.contains("- synthetic"));
        assert!(output.contains("- smoke"));
    }

    /// A fully valid model — the views parse against the typed contract, so the
    /// fixture must be one the runtime would admit. Mirrors the runtime's
    /// canonical `synthetic_model` test fixture.
    fn model_fixture() -> String {
        serde_json::json!({
            "modelVersion": nixfied_model::MODEL_VERSION,
            "toolchainId": nixfied_model::TOOLCHAIN_ID,
            "runtimeAbi": nixfied_model::runtime_abi(),
            "generator": {
                "name": "nixfied",
                "version": "1",
                "emitter": "nix/compiler/emit-model.nix"
            },
            "project": {
                "projectId": "view-test",
                "name": "View Test"
            },
            "target": {
                "system": "aarch64-darwin",
                "os": "darwin",
                "arch": "aarch64",
                "closureSystem": "aarch64-darwin"
            },
            "codebases": [{
                "codebaseId": "main",
                "logicalRoot": ".",
                "sourceMode": "live-workspace",
                "sourceIdentity": "live",
                "sourcePolicy": {
                    "dirtyPolicy": "warn",
                    "admissionFingerprintPolicy": "live-fingerprint"
                }
            }],
            "environments": {
                "dev": { "services": ["synthetic"], "tasks": ["smoke"] }
            },
            "slotPolicy": { "min": 0, "default": 0, "max": 0 },
            "placement": {
                "slotPlacements": {
                    "0": {
                        "slot": 0,
                        "candidatePorts": { "start": 42000, "end": 42063 }
                    }
                }
            },
            "state": {
                "markerIdentity": "nixfied-state",
                "stateEpoch": "1",
                "cleanupPolicy": "delete-on-clean",
                "persistence": "run-scoped"
            },
            "closures": {
                "synthetic-helper": {
                    "kind": "executable",
                    "storePath": "/nix/store/test-synthetic-helper",
                    "executable": "/nix/store/test-synthetic-helper/bin/synthetic-helper",
                    "targetSystem": "aarch64-darwin",
                    "operationBindings": [
                        "service.synthetic.start",
                        "service.synthetic.stop",
                        "task.smoke.run"
                    ],
                    "requiresExecutable": true,
                    "effects": ["process", "network-listener"]
                }
            },
            "execs": {
                "synthetic-helper": {
                    "closureId": "synthetic-helper",
                    "executable": "/nix/store/test-synthetic-helper/bin/synthetic-helper",
                    "args": [],
                    "env": {},
                    "codebaseId": "main",
                    "cwd": ".",
                    "stdin": "null",
                    "timeoutMs": 30000
                }
            },
            "services": {
                "synthetic": {
                    "lifecycle": {
                        "prepare": {
                            "operationId": "service.synthetic.prepare",
                            "execId": null,
                            "execArgs": [],
                            "terminal": { "success": "prepared", "failure": "failed" }
                        },
                        "start": {
                            "operationId": "service.synthetic.start",
                            "execId": "synthetic-helper",
                            "execArgs": ["service"],
                            "terminal": { "success": "spawned", "failure": "failed" }
                        },
                        "ready": {
                            "operationId": "service.synthetic.ready",
                            "probe": { "kind": "tcp", "timeoutMs": 250, "retryIntervalMs": 25, "maxAttempts": 40 },
                            "terminal": { "success": "ready", "failure": "not-ready" }
                        },
                        "health": {
                            "operationId": "service.synthetic.health",
                            "probe": { "kind": "tcp", "timeoutMs": 250, "retryIntervalMs": 25, "maxAttempts": 40 },
                            "terminal": { "success": "healthy", "failure": "unhealthy" }
                        },
                        "stop": {
                            "operationId": "service.synthetic.stop",
                            "signal": "TERM",
                            "timeoutMs": 5000,
                            "terminal": { "success": "stopped", "failure": "failed" }
                        },
                        "clean": {
                            "operationId": "service.synthetic.clean",
                            "terminal": { "success": "cleaned", "failure": "failed" }
                        }
                    },
                    "endpoint": { "endpointId": "synthetic-tcp", "host": "127.0.0.1" },
                    "connectsTo": [],
                    "stateRefs": ["slot"],
                    "logRefs": ["service.synthetic"],
                    "containment": "process-group",
                    "identity": {
                        "serviceAddressHash": "service-address",
                        "endpointIdentityHash": "endpoint",
                        "stateIdentityHash": "state",
                        "runtimeCompatibilityHash": "runtime",
                        "targetIdentityHash": "target"
                    }
                }
            },
            "tasks": {
                "smoke": {
                    "operationId": "task.smoke.run",
                    "execId": "synthetic-helper",
                    "args": ["task"],
                    "dependsOnServicesReady": ["synthetic"],
                    "exitPolicy": { "successCodes": [0] },
                    "artifactRefs": [],
                    "logRefs": ["task.smoke"],
                    "summaryRefs": ["summary"]
                }
            },
            "workflows": {},
            "docs": {
                "title": "View Test",
                "summary": "Generated from model.json."
            }
        })
        .to_string()
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).expect("test file should read")
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "nixfied-cli-test-{}-{}",
                std::process::id(),
                unique_suffix()
            ));
            std::fs::create_dir_all(&path).expect("temp dir should be created");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn unique_suffix() -> u128 {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic enough for tests")
            .as_nanos();
        let count = COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
        (now << 16) | count
    }
}
