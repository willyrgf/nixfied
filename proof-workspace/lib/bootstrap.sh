#!/usr/bin/env bash

set -euo pipefail

proof_log_info() {
  printf 'INFO: %s\n' "$1"
}

proof_log_ok() {
  printf 'OK: %s\n' "$1"
}

proof_log_error() {
  printf 'ERROR: %s\n' "$1" >&2
}

proof_require_dir() {
  local path="$1"
  [ -d "$path" ] || {
    proof_log_error "missing directory: $path"
    exit 1
  }
}

proof_require_file() {
  local path="$1"
  [ -f "$path" ] || {
    proof_log_error "missing file: $path"
    exit 1
  }
}

proof_write_seed_flake() {
  local target_root="$1"
  local repo_root="$2"
  local seed_flake="$target_root/flake.nix"

  proof_require_file "$seed_flake"

  sed -i.bak \
    "s|path:/tmp/nixfied-source-not-materialized|path:${repo_root}|g" \
    "$seed_flake"
  rm -f "$seed_flake.bak"
}

proof_materialize_project_files() {
  local target_root="$1"
  local repo_root="$2"

  mkdir -p "$target_root/nixfied/project"
  cp -R "$repo_root/nixfied/project/." "$target_root/nixfied/project/"
}

proof_init_git_baseline() {
  local target_root="$1"

  git -C "$target_root" init >/dev/null 2>&1
  git -C "$target_root" add .
  git -C "$target_root" \
    -c user.name=nixfied-proof \
    -c user.email=nixfied-proof@example.invalid \
    commit -m "proof workspace baseline" >/dev/null 2>&1
}

proof_bootstrap_seed_copy() {
  local target_root="$1"
  local repo_root="$2"
  local seed_root="$repo_root/proof-workspace/seed"

  proof_require_dir "$seed_root"

  rm -rf "$target_root"
  mkdir -p "$target_root"
  cp -R "$seed_root/." "$target_root/"
  chmod -R u+w "$target_root" >/dev/null 2>&1 || true

  proof_materialize_project_files "$target_root" "$repo_root"
  proof_write_seed_flake "$target_root" "$repo_root"
  proof_init_git_baseline "$target_root"

  proof_log_ok "seed-copy bootstrap ready at $target_root"
}

proof_bootstrap_install() {
  local target_root="$1"
  local repo_root="$2"

  rm -rf "$target_root"
  mkdir -p "$target_root"

  proof_log_info "running fully-qualified framework::install bootstrap"
  nix run "path:${repo_root}#framework::install" -- --vendor --target "$target_root" > "$target_root/.proof-install.out" 2>&1 || {
    proof_log_error "framework::install bootstrap failed"
    cat "$target_root/.proof-install.out" >&2
    exit 1
  }

  proof_materialize_project_files "$target_root" "$repo_root"
  proof_init_git_baseline "$target_root"

  proof_log_ok "install bootstrap ready at $target_root"
}

if [ "$#" -ge 1 ]; then
  mode="$1"
  shift

  case "$mode" in
    seed-copy)
      [ "$#" -eq 2 ] || {
        proof_log_error "usage: bootstrap.sh seed-copy <target-root> <repo-root>"
        exit 2
      }
      proof_bootstrap_seed_copy "$1" "$2"
      ;;
    install)
      [ "$#" -eq 2 ] || {
        proof_log_error "usage: bootstrap.sh install <target-root> <repo-root>"
        exit 2
      }
      proof_bootstrap_install "$1" "$2"
      ;;
    *)
      proof_log_error "unknown mode: $mode"
      exit 2
      ;;
  esac
fi
