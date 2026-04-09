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

proof_copy_seed_workspace() {
  local seed_root="$1"
  local target_root="$2"

  rm -rf "$target_root"
  mkdir -p "$target_root"
  cp -R "$seed_root/." "$target_root/"
  chmod -R u+w "$target_root" >/dev/null 2>&1 || true
}

proof_overlay_seed_workspace() {
  local seed_root="$1"
  local target_root="$2"
  local root_entry=""

  mkdir -p "$target_root"

  for root_entry in AGENTS.md README.md app docs fixtures; do
    if [ -e "$seed_root/$root_entry" ]; then
      rm -rf "$target_root/$root_entry"
      cp -R "$seed_root/$root_entry" "$target_root/$root_entry"
    fi
  done

  mkdir -p "$target_root/nixfied"
  rm -rf "$target_root/nixfied/project"
  if [ -d "$seed_root/nixfied/project" ]; then
    cp -R "$seed_root/nixfied/project" "$target_root/nixfied/project"
  fi

  rm -rf "$target_root/nixfied/local"
  if [ -d "$seed_root/nixfied/local" ]; then
    cp -R "$seed_root/nixfied/local" "$target_root/nixfied/local"
  fi

  chmod -R u+w "$target_root" >/dev/null 2>&1 || true
}

proof_assert_minimal_seed() {
  local seed_root="$1"
  local seed_nixfied_root="$seed_root/nixfied"
  local entry=""

  proof_require_dir "$seed_nixfied_root"

  for entry in "$seed_nixfied_root"/*; do
    [ -e "$entry" ] || continue
    case "$(basename "$entry")" in
      project|local)
        ;;
      *)
        proof_log_error "proof seed must keep only nixfied/project and nixfied/local; found $entry"
        exit 1
        ;;
    esac
  done
}

proof_rewrite_framework_input() {
  local target_root="$1"
  local repo_root="$2"
  local target_flake="$target_root/flake.nix"

  proof_require_file "$target_flake"

  if ! grep -Fq 'github:willyrgf/nixfied/dev' "$target_flake"; then
    proof_log_error "expected thin-style nixfied input in $target_flake"
    exit 1
  fi

  sed -i.bak \
    "s|github:willyrgf/nixfied/dev|path:${repo_root}|g" \
    "$target_flake"
  rm -f "$target_flake.bak"
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
  proof_assert_minimal_seed "$seed_root"

  proof_copy_seed_workspace "$seed_root" "$target_root"
  proof_rewrite_framework_input "$target_root" "$repo_root"
  proof_init_git_baseline "$target_root"

  proof_log_ok "seed-copy bootstrap ready at $target_root"
}

proof_bootstrap_install() {
  local target_root="$1"
  local repo_root="$2"
  local install_mode="${3:-vendor}"
  local -a install_args
  local proof_home
  local seed_root="$repo_root/proof-workspace/seed"

  proof_require_dir "$seed_root"

  case "$install_mode" in
    vendor)
      install_args=(--vendor)
      ;;
    thin)
      install_args=()
      ;;
    *)
      proof_log_error "unknown install mode: $install_mode"
      exit 2
      ;;
  esac

  rm -rf "$target_root"
  mkdir -p "$target_root"
  proof_home="$target_root/.proof-home"
  mkdir -p "$proof_home/.cache"

  proof_log_info "running fully-qualified framework::install bootstrap mode=$install_mode"
  HOME="$proof_home" XDG_CACHE_HOME="$proof_home/.cache" \
    nix run "path:${repo_root}#framework::install" -- "${install_args[@]}" --target "$target_root" > "$target_root/.proof-install.out" 2>&1 || {
    proof_log_error "framework::install bootstrap failed"
    cat "$target_root/.proof-install.out" >&2
    exit 1
  }

  if [ "$install_mode" = "thin" ]; then
    proof_rewrite_framework_input "$target_root" "$repo_root"
  fi
  proof_overlay_seed_workspace "$seed_root" "$target_root"
  proof_init_git_baseline "$target_root"

  proof_log_ok "install bootstrap ready mode=$install_mode at $target_root"
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
      [ "$#" -ge 2 ] && [ "$#" -le 3 ] || {
        proof_log_error "usage: bootstrap.sh install <target-root> <repo-root> [thin|vendor]"
        exit 2
      }
      proof_bootstrap_install "$1" "$2" "${3:-vendor}"
      ;;
    *)
      proof_log_error "unknown mode: $mode"
      exit 2
      ;;
  esac
fi
