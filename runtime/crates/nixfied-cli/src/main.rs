use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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
      # The generated surface: the reserved control apps (`.#run` / `.#ps` /
      # `.#down` / `.#clean` / `.#model-check`) plus one app per task id exported in
      # `nixfied.surface.verbs`.
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

  # The synthetic adapter contributes the `smoke` task (it pings the
  # service); exporting it below makes it a flake app: `nix run .#smoke`.
  # Add your own leaf tasks (lint/test), compose them into composite tasks
  # (kind = "composite", steps = ...), and export the ones that form your
  # public surface. `nix run .#model-check` checks the model admits;
  # `nix run .#run -- --task <id>` runs any declared task.
  nixfied.surface.verbs = [ "smoke" ];
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

# The generated surface: the reserved control apps plus one app per exported
# task id (`nixfied.surface.verbs`).
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
    print_install_usage();
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
        // The scaffold wires the generated surface: the reserved control apps
        // plus the exported verbs.
        assert!(flake.contains("projectApps ./nixfied.nix"));
        let module = read(&root.join("nixfied.nix"));
        assert!(module.contains("nixfied.project.projectId = \"install-proof\""));
        // The scaffold ships a runnable service so `nix build .#model` builds,
        // and exports the starter task so `nix run .#smoke` works out of the
        // box.
        assert!(module.contains("imports = [ adapters.synthetic ];"));
        assert!(module.contains("nixfied.surface.verbs = [ \"smoke\" ];"));
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
