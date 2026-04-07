{ pkgs, ... }:
let
  materialize = import ../lib/materialize.nix {
    inherit pkgs;
    repoRoot = ../..;
  };
in
pkgs.runCommand "proof-workspace-scenario-5-wrapper-roundtrip"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.git
      pkgs.gnugrep
      pkgs.gnused
      pkgs.nix
    ];
  }
  ''
    set -euo pipefail
    ${materialize.shellPrelude}

    run_wrapper_checked() {
      local workspace_root="$1"
      local out_file="$2"
      shift 2
      local wrapper_home="$workspace_root/.proof-home"
      mkdir -p "$wrapper_home/.cache"
      set +e
      HOME="$wrapper_home" XDG_CACHE_HOME="$wrapper_home/.cache" "$@" > "$out_file" 2>&1
      local rc="$?"
      set -e
      if [ "$rc" -ne 0 ]; then
        cat "$out_file" 2>/dev/null || true
        proof_fail "wrapper command failed rc=$rc"
      fi
    }

    thin_workspace="$TMPDIR/proof-install-thin"
    vendor_workspace="$TMPDIR/proof-install-vendor"

    proof_workspace_bootstrap_install "$thin_workspace" thin
    proof_workspace_bootstrap_install "$vendor_workspace" vendor

    proof_require_dir "$thin_workspace/.git"
    proof_require_file "$thin_workspace/flake.nix"
    proof_require_dir "$vendor_workspace/.git"
    proof_require_file "$vendor_workspace/flake.nix"

    run_wrapper_checked \
      "$thin_workspace" \
      "$TMPDIR/scenario5.thin.validate.out" \
      nix run "path:$thin_workspace#validate-env"
    proof_require_contains "$TMPDIR/scenario5.thin.validate.out" "OK: environment is valid"

    run_wrapper_checked \
      "$vendor_workspace" \
      "$TMPDIR/scenario5.vendor.validate.out" \
      nix run "path:$vendor_workspace#validate-env"
    proof_require_contains "$TMPDIR/scenario5.vendor.validate.out" "OK: environment is valid"

    run_wrapper_checked \
      "$thin_workspace" \
      "$TMPDIR/scenario5.thin.task.out" \
      env \
      REGISTRY_ROOT="$TMPDIR/thin-registry" \
      CI_ARTIFACTS_ROOT="$TMPDIR/thin-artifacts" \
      nix run "path:$thin_workspace#run-task" -- task.test.isolation.unit
    proof_require_contains "$TMPDIR/scenario5.thin.task.out" "OK: isolation probe complete"

    run_wrapper_checked \
      "$vendor_workspace" \
      "$TMPDIR/scenario5.vendor.task.out" \
      env \
      REGISTRY_ROOT="$TMPDIR/vendor-registry" \
      CI_ARTIFACTS_ROOT="$TMPDIR/vendor-artifacts" \
      nix run "path:$vendor_workspace#run-task" -- task.test.isolation.unit
    proof_require_contains "$TMPDIR/scenario5.vendor.task.out" "OK: isolation probe complete"

    project_file="$vendor_workspace/nixfied/project/module.nix"
    local_file="$vendor_workspace/nixfied/local/default.nix"
    vendored_metadata_file="$vendor_workspace/nixfied/VENDORED.txt"

    proof_require_file "$project_file"
    proof_require_file "$local_file"
    proof_require_file "$vendored_metadata_file"
    proof_require_contains "$vendored_metadata_file" "Framework source revision (install/upgrade):"

    printf '\n# PROOF_USER_PROJECT_MARKER\n' >> "$project_file"
    printf '\n# PROOF_USER_LOCAL_MARKER\n' >> "$local_file"
    printf '\n# PROOF_FRAMEWORK_VENDORED_MARKER\n' >> "$vendored_metadata_file"

    run_wrapper_checked \
      "$vendor_workspace" \
      "$TMPDIR/scenario5.vendor.upgrade.out" \
      nix run "path:$vendor_workspace#framework::upgrade" -- --target "$vendor_workspace"

    proof_require_contains "$project_file" "PROOF_USER_PROJECT_MARKER"
    proof_require_contains "$local_file" "PROOF_USER_LOCAL_MARKER"
    if ${pkgs.gnugrep}/bin/grep -Fq "PROOF_FRAMEWORK_VENDORED_MARKER" "$vendored_metadata_file"; then
      proof_fail "framework-owned vendored metadata marker should be removed by upgrade"
    fi

    proof_require_file "$vendored_metadata_file"
    proof_require_contains "$vendored_metadata_file" "Framework source revision (install/upgrade):"
    if ! ${pkgs.gnugrep}/bin/grep -Eq '^- [0-9a-f]{7,}(-dirty)?$' "$vendored_metadata_file"; then
      proof_fail "vendored metadata must contain a framework source revision entry"
    fi

    run_wrapper_checked \
      "$vendor_workspace" \
      "$TMPDIR/scenario5.vendor.post-upgrade.validate.out" \
      nix run "path:$vendor_workspace#validate-env"
    proof_require_contains "$TMPDIR/scenario5.vendor.post-upgrade.validate.out" "OK: environment is valid"

    echo "OK: proof workspace scenario 5 wrapper roundtrip passed" > "$out"
  ''
