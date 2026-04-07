{
  pkgs,
  repoRoot ? ../..,
}:
let
  inherit (pkgs) lib;
  repoRootPath = builtins.toString repoRoot;
  seedRootPath = builtins.toString ../seed;
  assertScriptPath = builtins.toString ./assert.sh;
in
{
  shellPrelude = ''
    set -euo pipefail

    export PROOF_WORKSPACE_REPO_ROOT=${lib.escapeShellArg repoRootPath}
    export PROOF_WORKSPACE_SEED_ROOT=${lib.escapeShellArg seedRootPath}
    export PROOF_WORKSPACE_ASSERT_SH=${lib.escapeShellArg assertScriptPath}

    # shellcheck source=/dev/null
    source "$PROOF_WORKSPACE_ASSERT_SH"

    proof_workspace_copy_seed() {
      local target_root="$1"
      rm -rf "$target_root"
      mkdir -p "$target_root"
      cp -R "$PROOF_WORKSPACE_SEED_ROOT/." "$target_root/"
      chmod -R u+w "$target_root" >/dev/null 2>&1 || true
    }

    proof_workspace_materialize_project_files() {
      local target_root="$1"
      mkdir -p "$target_root/nixfied/project"
      cp -R "$PROOF_WORKSPACE_REPO_ROOT/nixfied/project/." "$target_root/nixfied/project/"
    }

    proof_workspace_init_git_baseline() {
      local target_root="$1"
      ${pkgs.git}/bin/git -C "$target_root" init >/dev/null 2>&1
      ${pkgs.git}/bin/git -C "$target_root" add .
      ${pkgs.git}/bin/git -C "$target_root" \
        -c user.name=nixfied-proof \
        -c user.email=nixfied-proof@example.invalid \
        commit -m "proof workspace baseline" >/dev/null 2>&1
    }
  '';
}
