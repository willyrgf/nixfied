# The generated surface nixfied derives for an adopting project from its
# module (VERB-1, the corrected split of SURFACE-1):
#
# - the **reserved control namespace**, framework-owned: `run`, `ps`, `down`,
#   `clean`, and `model-check` (admission sanity, freeing common adopter verbs);
# - the **project verbs**, adopter-owned: one app per task id exported in
#   `nixfied.surface.verbs` (`.#check` -> `runtime run --task check`).
#
# Everything runs through the nix-built runtime against the model store path
# baked in at eval time; no nix is invoked at run time, so SEAM-1 holds.
# `.#gate` is deliberately absent: that is the framework proving its own
# runtime.
{
  pkgs,
  lib,
  runtime,
  model,
  config,
}:
let
  runtimeBin = "${runtime}/bin/nixfied-runtime";
  modelJson = "${model}/model.json";
  verbs = config.nixfied.surface.verbs;
  mkApp =
    name: text:
    let
      drv = pkgs.writeShellApplication { inherit name text; };
    in
    {
      type = "app";
      program = "${drv}/bin/${name}";
    };
  # `validate.nix` already proved each verb names a declared task and avoids
  # the reserved namespace; this projection just derives the apps.
  verbApps = builtins.listToAttrs (
    map (verb: {
      name = verb;
      value = mkApp verb ''exec "${runtimeBin}" run --model "${modelJson}" --task "${verb}" "$@"'';
    }) verbs
  );
in
{
  # Run one selected task (`nix run .#run -- --task <id>`): its derived
  # service union starts eagerly, then the flattened nodes execute. With no
  # selection the runtime refuses and lists the declared tasks.
  run = mkApp "run" ''exec "${runtimeBin}" run --model "${modelJson}" "$@"'';

  # Admission sanity: the model is well-formed and admits (cheap, no
  # execution). Named `model-check` so `check` stays free for adopters.
  model-check = mkApp "model-check" ''exec "${runtimeBin}" check --model "${modelJson}"'';

  # Recovery/control surface over the project's slots: observe registry-owned
  # processes (reconciling stale evidence), stop everything the runtime owns,
  # and remove the marker-gated slot state. Extra args are forwarded
  # (e.g. `nix run .#down -- --slot 1`).
  ps = mkApp "ps" ''exec "${runtimeBin}" ps --model "${modelJson}" "$@"'';
  down = mkApp "down" ''exec "${runtimeBin}" down --model "${modelJson}" "$@"'';
  clean = mkApp "clean" ''exec "${runtimeBin}" clean --model "${modelJson}" "$@"'';
}
// verbApps
