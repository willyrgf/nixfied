{
  pkgs,
  repoRoot ? ../..,
}:
let
  inherit (pkgs) lib;
  repoRootPath = builtins.toString repoRoot;
  assertScriptPath = builtins.toString ./assert.sh;
  bootstrapScriptPath = builtins.toString ./bootstrap.sh;
in
{
  shellPrelude = ''
    set -euo pipefail

    export PROOF_WORKSPACE_REPO_ROOT=${lib.escapeShellArg repoRootPath}
    export PROOF_WORKSPACE_ASSERT_SH=${lib.escapeShellArg assertScriptPath}
    export PROOF_WORKSPACE_BOOTSTRAP_SH=${lib.escapeShellArg bootstrapScriptPath}

    # shellcheck source=/dev/null
    source "$PROOF_WORKSPACE_ASSERT_SH"
    # shellcheck source=/dev/null
    source "$PROOF_WORKSPACE_BOOTSTRAP_SH"

    proof_workspace_bootstrap_seed_copy() {
      local target_root="$1"
      proof_bootstrap_seed_copy "$target_root" "$PROOF_WORKSPACE_REPO_ROOT"
    }

    proof_workspace_bootstrap_install() {
      local target_root="$1"
      local install_mode="''${2:-vendor}"
      proof_bootstrap_install "$target_root" "$PROOF_WORKSPACE_REPO_ROOT" "$install_mode"
    }
  '';
}
