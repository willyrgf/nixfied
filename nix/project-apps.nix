# The verification + run surface nixfied generates for an adopting project from
# its compiled model. For an adopter, verification *is* a workflow — their tests
# are tasks — so these run through the nix-built runtime against the model store
# path baked in at eval time; no nix is invoked at run time, so SEAM-1 holds.
#
# `.#gate` is deliberately absent: that is the framework proving its own runtime.
# A project's acceptance proof is just one of its workflows (run via `.#run --
# --workflow <name>`).
{
  pkgs,
  runtime,
  model,
}:
let
  runtimeBin = "${runtime}/bin/nixfied-runtime";
  modelJson = "${model}/model.json";
  mkApp =
    name: text:
    let
      drv = pkgs.writeShellApplication { inherit name text; };
    in
    {
      type = "app";
      program = "${drv}/bin/${name}";
    };
in
{
  # Start the environment's services and run its tasks. Extra args are forwarded
  # (e.g. `nix run .#run -- --workflow release`).
  run = mkApp "run" ''exec "${runtimeBin}" run --model "${modelJson}" "$@"'';

  # Admission sanity: the model is well-formed and admits (cheap, no execution).
  check = mkApp "check" ''exec "${runtimeBin}" check --model "${modelJson}"'';

  # Run the project's `test` workflow — its lint/test/e2e tasks.
  test = mkApp "test" ''exec "${runtimeBin}" run --model "${modelJson}" --workflow test "$@"'';

  # Fail-fast: admit, then run the test workflow.
  ci = mkApp "ci" ''
    "${runtimeBin}" check --model "${modelJson}"
    "${runtimeBin}" run --model "${modelJson}" --workflow test
  '';
}
