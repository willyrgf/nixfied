# Nixfied install surface, packaged as a shell application.
# The scaffolding script is embedded here (literal `${...}` is escaped as
# `''${...}` for the Nix indented string); `nix run .#install` runs it.
{ pkgs }:
pkgs.writeShellApplication {
  name = "nixfied-install";
  runtimeInputs = [ pkgs.coreutils ];
  text = ''
set -euo pipefail

default_nixfied_url="github:willyrgf/nixfied"
root="."
project_id=""
project_name=""
nixfied_url="$default_nixfied_url"

usage() {
  echo 'usage: nixfied install [--root PATH] [--project-id ID] [--name NAME] [--nixfied-url URL]'
}

take_value() {
  local flag="$1"
  shift
  if [[ $# -eq 0 || -z "$1" || "$1" == --* ]]; then
    echo "missing $flag value" >&2
    exit 2
  fi
  printf '%s' "$1"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)
      root="$(take_value "$1" "''${2-}")"
      shift 2
      ;;
    --project-id)
      project_id="$(take_value "$1" "''${2-}")"
      shift 2
      ;;
    --name)
      project_name="$(take_value "$1" "''${2-}")"
      shift 2
      ;;
    --nixfied-url)
      nixfied_url="$(take_value "$1" "''${2-}")"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "unknown install argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

infer_name() {
  local path="$1"
  if [[ "$path" == "." ]]; then
    path="$PWD"
  fi
  path="''${path%/}"
  path="''${path##*/}"
  if [[ -z "$path" ]]; then
    echo "could not infer project name from --root" >&2
    exit 2
  fi
  printf '%s' "$path"
}

if [[ -z "$project_id" ]]; then
  project_id="$(infer_name "$root")"
fi
if [[ -z "$project_name" ]]; then
  project_name="$(infer_name "$root")"
fi

if [[ ! "$project_id" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]; then
  echo "invalid project id \"$project_id\"; expected [A-Za-z0-9][A-Za-z0-9._-]*" >&2
  exit 2
fi

nix_escape() {
  local value="$1"
  value="''${value//\\/\\\\}"
  value="''${value//\"/\\\"}"
  printf '%s' "$value"
}

nixfied_url_escaped="$(nix_escape "$nixfied_url")"
project_id_escaped="$(nix_escape "$project_id")"
project_name_escaped="$(nix_escape "$project_name")"

print_flake_snippet() {
  cat <<EOF
inputs.nixfied.url = "$nixfied_url_escaped";

packages.\''${system}.model = nixfied.lib.\''${system}.compileModel ./nixfied.nix;
EOF
}

print_nixfied_module() {
  cat <<EOF
{ adapters, ... }:
{
  # The synthetic adapter is a runnable starter service. Replace it with your
  # own service/task declarations or another adapter (e.g. adapters.postgres).
  imports = [ adapters.synthetic ];

  nixfied.project.projectId = "$project_id_escaped";
  nixfied.project.name = "$project_name_escaped";
  nixfied.codebases.main.logicalRoot = ".";
}
EOF
}

if [[ -e "$root/flake.nix" ]]; then
  {
    echo "refusing to modify existing flake.nix in $root"
    echo "No files were changed."
    echo
    echo "Add this input and package wiring manually:"
    echo
    print_flake_snippet
    echo "Then create nixfied.nix if it does not exist:"
    echo
    print_nixfied_module
  } >&2
  exit 3
fi

mkdir -p "$root"

cat >"$root/flake.nix" <<EOF
{
  description = "Nixfied project";

  inputs = {
    nixfied.url = "$nixfied_url_escaped";
  };

  outputs =
    { self, nixfied }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
      forAllSystems = f: builtins.listToAttrs (
        map (system: {
          name = system;
          value = f system;
        }) systems
      );
    in
    {
      packages = forAllSystems (system: {
        default = self.packages.\''${system}.model;
        model = nixfied.lib.\''${system}.compileModel ./nixfied.nix;
      });
    };
}
EOF

created="flake.nix"
skipped=""
if [[ -e "$root/nixfied.nix" ]]; then
  skipped="nixfied.nix"
else
  print_nixfied_module >"$root/nixfied.nix"
  created="$created, nixfied.nix"
fi

echo "Installed Nixfied scaffold in $root"
echo "projectId: $project_id"
echo "name: $project_name"
echo "created: $created"
if [[ -n "$skipped" ]]; then
  echo "skipped: $skipped"
fi
echo "next: nix build .#model"
  '';
}
