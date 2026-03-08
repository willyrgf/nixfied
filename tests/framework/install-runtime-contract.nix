{ pkgs }:
let
  installSource = builtins.readFile ../../nixfied/.framework/internal/install.nix;
  runtimeSource = builtins.readFile ../../nixfied/.framework/internal/install-runtime.nix;
  promptPlanSource = builtins.readFile ../../nixfied/.framework/internal/prompt-plan.nix;
in
assert pkgs.lib.hasInfix "promptPlanScript = import ./prompt-plan.nix" installSource;
assert pkgs.lib.hasInfix "installRuntime = import ./install-runtime.nix" installSource;
assert pkgs.lib.hasInfix "source \${installRuntime}" installSource;
assert !(pkgs.lib.hasInfix "while [ \"$#\" -gt 0 ]" installSource);
assert pkgs.lib.hasInfix "parse_install_args()" runtimeSource;
assert pkgs.lib.hasInfix "resolve_install_repo_root()" runtimeSource;
assert pkgs.lib.hasInfix "maybe_reenter_install_worktree()" runtimeSource;
assert pkgs.lib.hasInfix "resolve_install_branch_context()" runtimeSource;
assert pkgs.lib.hasInfix "maybe_confirm_install_overwrite()" runtimeSource;
assert pkgs.lib.hasInfix "maybe_generate_prompt_plan()" runtimeSource;
assert pkgs.lib.hasInfix "Create a PROMPT PLAN in Markdown" promptPlanSource;
pkgs.runCommand "install-runtime-contract" { } ''
  echo "OK: install runtime policy is split into focused helpers" > "$out"
''
