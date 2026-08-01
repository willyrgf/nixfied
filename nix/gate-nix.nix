# Nix-layer tests: checks that exercise the Nix compiler and install tooling.
# These stay in bash because they perform open-ended Nix builds and external
# fetches; expressing them as runtime tasks would exceed the bounded role of
# those framework tasks and blur which layer is under test.
{ pkgs, runtime }:
pkgs.writeShellApplication {
  name = "nixfied-gate-nix";
  runtimeInputs = [
    pkgs.nix
    pkgs.git
    pkgs.jq
    pkgs.coreutils
  ];
  text = ''
    checkout="''${NIXFIED_GATE_CHECKOUT:-$PWD}"
    dirty="''${NIXFIED_GATE_DIRTY:-}"

    for arg in "$@"; do
      case "$arg" in
        --dirty) dirty=1 ;;
        *) echo "gate-nix: unknown argument: $arg (supported: --dirty)" >&2; exit 1 ;;
      esac
    done

    rt="${runtime}/bin/nixfied-runtime"

    fail() {
      echo "  GATE FAIL: $*" >&2
      exit 1
    }

    framework_help() {
      local current_system help_dir help_state lock_before expected actual
      current_system=$(nix eval --impure --raw --expr builtins.currentSystem)
      help_dir=$(mktemp -d)
      help_state="$help_dir/state"
      lock_before=$(sha256sum "$checkout/flake.lock")
      expected=$(nix eval --no-write-lock-file --raw "$checkout#apps.$current_system" \
        --apply "$(<"$checkout/nix/help-renderer.nix")") \
        || fail "framework help: final root app metadata did not render"
      actual=$(
        cd "$help_dir"
        NIXFIED_STATE_DIR="$help_state" nix run --no-write-lock-file "$checkout#help"
      ) || fail "framework help: explicit checkout invocation failed"
      [ "$actual" = "$expected" ] \
        || fail "framework help: output did not match the final root app metadata"
      for flag in -h --help; do
        actual=$(
          cd "$help_dir"
          NIXFIED_STATE_DIR="$help_state" nix run --no-write-lock-file "$checkout#help" -- "$flag"
        ) || fail "framework help: $flag failed"
        [ "$actual" = "$expected" ] \
          || fail "framework help: $flag changed the catalog"
      done
      if (
        cd "$help_dir"
        nix run --no-write-lock-file "$checkout#help" -- unexpected
      ) >/dev/null 2>&1; then
        fail "framework help: accepted an unknown argument"
      fi
      if (
        cd "$help_dir"
        nix run --no-write-lock-file "$checkout#help" -- one two
      ) >/dev/null 2>&1; then
        fail "framework help: accepted multiple arguments"
      fi
      [ ! -e "$help_state" ] || fail "framework help: materialized runtime state"
      [ ! -e "$help_dir/flake.lock" ] || fail "framework help: wrote a caller lock file"
      [ "$(sha256sum "$checkout/flake.lock")" = "$lock_before" ] \
        || fail "framework help: modified the framework lock file"
      nix eval --impure --raw --expr "
        let render = import $checkout/nix/help-renderer.nix;
        in render {
          a = { program = \"/a\"; meta.description = \"A\"; };
          bbb = { program = \"/bbb\"; meta.description = \"B\"; };
        }
      " >"$help_dir/renderer.actual" || fail "framework help: renderer golden did not evaluate"
      printf 'Available commands:\n\n  a    A\n  bbb  B\n' >"$help_dir/renderer.expected"
      [ "$(sha256sum <"$help_dir/renderer.actual")" = "$(sha256sum <"$help_dir/renderer.expected")" ] \
        || fail "framework help: renderer order or layout drifted"
      printf '%s\n' "$actual" | grep -Eq "^  help +List this flake's runnable commands$" \
        || fail "framework help: catalog omitted its own app"
      if printf '%s\n' "$actual" | grep -Fq "nix run .#"; then
        fail "framework help: catalog baked a caller-relative invocation"
      fi
      rm -rf "$help_dir"
    }

    t0=$SECONDS
    echo "  framework help (final root app metadata from any cwd)" >&2
    framework_help
    printf '  framework_help: %ds\n' "$((SECONDS - t0))" >&2

    # Nix-layer validation must fail before an invalid model or app catalog can
    # become executable. Batch the negative expressions into one evaluation.
    reject_invalid_evaluations() {
      local unexpected
      if ! unexpected=$(nix eval --json --impure --expr "import ${./gate-nix-negatives.nix} { checkout = $checkout; }"); then
        fail "negative: batched invalid evaluation failed"
      fi
      if [ "$unexpected" != "[]" ]; then
        echo "  unexpected successful negative cases:" >&2
        echo "$unexpected" | jq -r '.[] | "    - " + .' >&2
        fail "negative: invalid expressions succeeded instead of failing at evaluation"
      fi
    }

    t0=$SECONDS
    echo "  negative (invalid models and app metadata must fail at nix evaluation)" >&2
    reject_invalid_evaluations
    printf '  reject_invalid_evaluations: %ds\n' "$((SECONDS - t0))" >&2

    echo "  positive (immutable source dirtyPolicy reject admits)" >&2
    t0=$SECONDS
    immutable_model=$(nix build --no-link --print-out-paths --impure --expr \
      "let flake = builtins.getFlake (toString $checkout); compileModel = (builtins.getAttr builtins.currentSystem flake.lib).compileModel; in compileModel ({ ... }: { imports = [ $checkout/examples/minimal/nixfied.nix ]; nixfied.codebases.main.sourceMode = \"snapshot\"; nixfied.codebases.main.sourceIdentity = builtins.path { path = $checkout/examples/minimal; name = \"nixfied-minimal-source\"; }; nixfied.codebases.main.dirtyPolicy = \"reject\"; })") \
      || fail "immutable source: model build failed"
    "$rt" check --model "$immutable_model/model.json" >/dev/null \
      || fail "immutable source: runtime check failed"
    printf '  immutable_source: %ds\n' "$((SECONDS - t0))" >&2

    echo "  adoption (#install + #upgrade against a throwaway repo)" >&2
    t0=$SECONDS
    if [ -n "$dirty" ]; then
      pin="path:$checkout"
      echo "    pin: $pin (--dirty: every run re-derives the closure)" >&2
    else
      pin="git+file://$checkout?rev=$(git -C "$checkout" rev-parse HEAD)"
      # Nix refuses to fetch from shallow clones (CI checkouts) unless told.
      if [ "$(git -C "$checkout" rev-parse --is-shallow-repository)" = true ]; then
        pin="$pin&shallow=1"
      fi
      if ! git -C "$checkout" diff --quiet HEAD 2>/dev/null; then
        echo "    pin: HEAD — uncommitted changes are NOT exercised here (use --dirty)" >&2
      fi
    fi
    current_system=$(nix eval --impure --raw --expr builtins.currentSystem)
    path_project=$(mktemp -d)
    nix run "$checkout#install" -- \
      --root "$path_project" --project-id path-adopt --name path-adopt --nixfied-url "$pin" \
      >/dev/null || fail "adoption: non-Git install failed"
    nix flake lock "$path_project" || fail "adoption: non-Git scaffold lock failed"
    path_lock_before=$(sha256sum "$path_project/flake.lock")
    path_help=$(cd "$path_project" && nix run --no-write-lock-file .#help) \
      || fail "adoption: non-Git contextual help failed"
    printf '%s\n' "$path_help" | grep -Eq "^  smoke +Run the starter smoke test$" \
      || fail "adoption: non-Git contextual help omitted the exported task"
    [ "$(sha256sum "$path_project/flake.lock")" = "$path_lock_before" ] \
      || fail "adoption: non-Git contextual help modified the lock file"
    ${pkgs.gnused}/bin/sed -i \
      '/^  nixfied.surface.verbs.smoke = /c\  nixfied.surface.verbs = { };' \
      "$path_project/nixfied.nix"
    empty_surface_apps=$(nix eval --no-write-lock-file --json "$path_project#apps.$current_system") \
      || fail "adoption: empty surface app metadata did not evaluate"
    printf '%s\n' "$empty_surface_apps" | jq -e '
      (keys | sort) == ["clean", "down", "help", "model-check", "ps", "run"]
    ' >/dev/null || fail "adoption: empty surface unexpectedly exported a task app"
    rm -rf "$path_project"

    project_repo=$(mktemp -d)
    project="$project_repo/adopter"
    mkdir "$project"
    git -C "$project_repo" init -q
    git -C "$project" config user.email gate@nixfied
    git -C "$project" config user.name "nixfied gate"
    install_output=$(nix run "$checkout#install" -- \
      --root "$project" --project-id adopt --name adopt --nixfied-url "$pin" \
    ) || fail "adoption: install failed"
    printf '%s\n' "$install_output" \
      | grep -Fq "stage flake.nix and nixfied.nix when using Git, then run nix flake lock" \
      || fail "adoption: installer omitted the required Git lock ordering"
    git -C "$project" add flake.nix nixfied.nix
    if prelock_error=$(nix eval --no-write-lock-file --json "$project#apps.$current_system" 2>&1); then
      fail "adoption: projectApps accepted a project without flake.lock"
    fi
    printf '%s\n' "$prelock_error" | grep -Fq "project-root flake.nix, flake.lock, and nixfied.nix" \
      || fail "adoption: pre-lock evaluation failed for the wrong reason"
    nix flake lock "$project" || fail "adoption: scaffold lock failed"
    git -C "$project" add flake.lock
    git -C "$project" commit -q -m scaffold
    scaffold_metadata=$(cd "$project" && nix flake metadata --no-write-lock-file --json .) \
      || fail "adoption: scaffold metadata did not resolve"
    printf '%s\n' "$scaffold_metadata" | jq -e '.resolved.dir == "adopter"' >/dev/null \
      || fail "adoption: Git subflake did not exercise resolved.dir source identity"
    scaffold_apps=$(nix eval --no-write-lock-file --json "$project#apps.$current_system") \
      || fail "adoption: untouched scaffold app metadata did not evaluate"
    printf '%s\n' "$scaffold_apps" | jq -e '
      (keys | sort) == ["clean", "down", "help", "model-check", "ps", "run", "smoke"]
      and all(.[];
        (.program | type == "string" and length > 0)
        and (.meta.description | type == "string" and length > 0)
      )
      and .smoke.meta.description == "Run the starter smoke test"
    ' >/dev/null || fail "adoption: untouched scaffold app namespace was incomplete"
    scaffold_help=$(cd "$project" && nix run --no-write-lock-file .#help) \
      || fail "adoption: untouched scaffold help failed"
    printf '%s\n' "$scaffold_help" | grep -Eq "^  smoke +Run the starter smoke test$" \
      || fail "adoption: untouched scaffold help omitted the exported task"
    if printf '%s\n' "$scaffold_help" | grep -Eq '^  merged +'; then
      fail "adoption: untouched scaffold unexpectedly contained the merge fixture"
    fi
    if nix run "$checkout#install" -- --root "$project" >/dev/null 2>&1; then
      fail "adoption: re-running install did not refuse an existing flake.nix"
    fi
    printf '%s\n' \
      '{ lib, ... }: { nixfied.surface.verbs.smoke = lib.mkDefault "Reusable default smoke"; }' \
      >"$project/reusable.nix"
    ${pkgs.gnused}/bin/sed -i \
      's@^  imports = \[ adapters.synthetic \];@  imports = [ adapters.synthetic ./reusable.nix ];@' \
      "$project/nixfied.nix"
    ${pkgs.gnused}/bin/sed -i \
      's@^  nixfied.surface.verbs.smoke = .*@  nixfied.surface.verbs = { smoke = "Run the starter smoke test"; "composite-smoke" = "Run the composite smoke test"; }; nixfied.tasks.composite-smoke = { kind = "composite"; steps.only.task = "smoke"; };@' \
      "$project/nixfied.nix"
    git -C "$project" add nixfied.nix reusable.nix
    git -C "$project" commit -q -m surface
    surface_apps=$(nix eval --json "$project#apps.$current_system") \
      || fail "adoption: leaf/composite surface app metadata did not evaluate"
    printf '%s\n' "$surface_apps" | jq -e '
      (keys | sort) == ["clean", "composite-smoke", "down", "help", "model-check", "ps", "run", "smoke"]
      and .smoke.meta.description == "Run the starter smoke test"
      and .["composite-smoke"].meta.description == "Run the composite smoke test"
    ' >/dev/null || fail "adoption: leaf/composite app descriptions were not copied exactly"
    surface_help=$(cd "$project" && nix run --no-write-lock-file .#help) \
      || fail "adoption: leaf/composite contextual help failed"
    printf '%s\n' "$surface_help" | grep -Eq "^  smoke +Run the starter smoke test$" \
      || fail "adoption: contextual help omitted the exact leaf description"
    printf '%s\n' "$surface_help" | grep -Eq "^  composite-smoke +Run the composite smoke test$" \
      || fail "adoption: contextual help omitted the exact composite description"
    ${pkgs.gnused}/bin/sed -i \
      '/^      apps = forAllSystems /c\      apps = forAllSystems (system: let generated = (builtins.getAttr system nixfied.lib).projectApps ./nixfied.nix; in generated // { smoke = generated.smoke // { meta.description = "Overridden adopter verb"; }; merged = generated.run // { meta.description = "Merged adopter app"; }; });' \
      "$project/flake.nix"
    git -C "$project" add flake.nix
    git -C "$project" commit -q -m merged-app
    apps_json=$(nix eval --json "$project#apps.$current_system") \
      || fail "adoption: generated app metadata did not evaluate"
    printf '%s\n' "$apps_json" | jq -e '
      all(.[];
        (.program | type == "string" and length > 0)
        and (.meta.description | type == "string" and length > 0)
      )
      and .smoke.meta.description == "Overridden adopter verb"
      and .["composite-smoke"].meta.description == "Run the composite smoke test"
      and has("help")
      and .merged.meta.description == "Merged adopter app"
    ' >/dev/null || fail "adoption: generated apps lack discoverable descriptions"
    model_dir="$(nix build --no-link --print-out-paths "$project#model")"
    model="$model_dir/model.json"
    if jq -e 'tostring | (contains("Run the starter smoke test") or contains("Run the composite smoke test") or contains("Overridden adopter verb"))' "$model" >/dev/null; then
      fail "adoption: surface descriptions entered model.json"
    fi
    if grep -Eq "Run the starter smoke test|Run the composite smoke test|Overridden adopter verb" "$model_dir/views/docs.md"; then
      fail "adoption: surface descriptions entered views/docs.md"
    fi
    st=$(mktemp -d)
    wk=$(mktemp -d)
    help_state="$st-help"
    help_lock_before=$(sha256sum "$project/flake.lock")
    project_help_expected=$(
      cd "$project"
      nix eval --no-write-lock-file --raw ".#apps.$current_system" \
        --apply "$(<"$checkout/nix/help-renderer.nix")"
    ) || fail "adoption: final project app metadata did not render"
    project_help=$(
      cd "$project"
      NIXFIED_STATE_DIR="$help_state" nix run --no-write-lock-file .#help
    ) || fail "adoption: contextual project help failed"
    [ "$project_help" = "$project_help_expected" ] \
      || fail "adoption: contextual help did not match final project app metadata"
    printf '%s\n' "$project_help" | grep -Eq "^  help +List this flake's runnable commands$" \
      || fail "adoption: contextual help omitted itself"
    printf '%s\n' "$project_help" | grep -Eq "^  smoke +Overridden adopter verb$" \
      || fail "adoption: contextual help ignored a final app metadata override"
    printf '%s\n' "$project_help" | grep -Eq "^  composite-smoke +Run the composite smoke test$" \
      || fail "adoption: contextual help lost the composite description"
    printf '%s\n' "$project_help" | grep -Eq '^  merged +Merged adopter app$' \
      || fail "adoption: contextual help omitted a post-projectApps merge"
    if wrong_context_error=$(
      cd "$checkout"
      nix run --no-write-lock-file "$project#help" 2>&1
    ); then
      fail "adoption: explicit help from another flake guessed the caller catalog"
    fi
    printf '%s\n' "$wrong_context_error" | grep -Fq "help: context mismatch" \
      || fail "adoption: explicit help failed for a reason other than context identity"
    if unresolved_context_error=$(
      cd "$wk"
      nix run --no-write-lock-file "$project#help" 2>&1
    ); then
      fail "adoption: explicit help from a non-flake directory guessed a catalog"
    fi
    printf '%s\n' "$unresolved_context_error" | grep -Fq "help: could not resolve the current flake source" \
      || fail "adoption: non-flake context failed for the wrong reason"
    for app in run model-check ps down clean smoke; do
      case "$app" in
        run) help_flag=--help; usage='nix run .#run' ;;
        model-check) help_flag=-h; usage='nix run .#model-check' ;;
        ps) help_flag=--help; usage='nix run .#ps' ;;
        down) help_flag=-h; usage='nix run .#down' ;;
        clean) help_flag=--help; usage='nix run .#clean' ;;
        smoke|composite-smoke) help_flag=-h; usage='nix run .#<verb>' ;;
      esac
      help_stdout="$wk/$app-help.stdout"
      ( cd "$wk" && NIXFIED_STATE_DIR="$help_state" nix run "$project#$app" -- "$help_flag" ) \
        >"$help_stdout" || fail "adoption: scaffolded $app $help_flag failed"
      grep -Fq "$usage" "$help_stdout" \
        || fail "adoption: scaffolded $app $help_flag omitted its usage"
    done
    [ ! -e "$help_state" ] || fail "adoption: generated app help materialized runtime state"
    [ "$(sha256sum "$project/flake.lock")" = "$help_lock_before" ] \
      || fail "adoption: generated app help modified the project lock file"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --task smoke --timeout-ms 60000 ) \
      >/dev/null || fail "adoption: scaffolded run failed"
    # The generated control surface must work against the same state.
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" nix run "$project#model-check" ) \
      >/dev/null || fail "adoption: scaffolded model-check failed"
    smoke_stdout="$st/smoke.stdout"
    smoke_stderr="$st/smoke.stderr"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" nix run "$project#smoke" -- --timeout-ms 60000 ) \
      >"$smoke_stdout" 2>"$smoke_stderr" || fail "adoption: scaffolded smoke verb failed"
    [ ! -s "$smoke_stdout" ] || fail "adoption: scaffolded smoke verb wrote JSON stdout in summary mode"
    grep -q "  result: ok 1 passed, 0 failed in " "$smoke_stderr" \
      || fail "adoption: scaffolded smoke verb did not print a concise result"
    grep -q "  run-summary: " "$smoke_stderr" \
      || fail "adoption: scaffolded smoke verb did not print the run summary path"
    grep -q "  logs: " "$smoke_stderr" \
      || fail "adoption: scaffolded smoke verb did not print the logs path"
    composite_stdout="$st/composite.stdout"
    composite_stderr="$st/composite.stderr"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" nix run "$project#composite-smoke" -- --timeout-ms 60000 ) \
      >"$composite_stdout" 2>"$composite_stderr" || fail "adoption: scaffolded composite smoke verb failed"
    [ ! -s "$composite_stdout" ] || fail "adoption: scaffolded composite smoke verb wrote JSON stdout in summary mode"
    grep -q "  result: ok 1 passed, 0 failed in " "$composite_stderr" \
      || fail "adoption: scaffolded composite smoke verb did not print a concise result"
    smoke_json_stdout="$st/smoke-json.stdout"
    smoke_json_stderr="$st/smoke-json.stderr"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" nix run "$project#smoke" -- --timeout-ms 60000 --json ) \
      >"$smoke_json_stdout" 2>"$smoke_json_stderr" || fail "adoption: scaffolded smoke verb --json failed"
    jq -e '.task.success == true and (.durationMs >= 0) and (.runSummaryPath | type == "string")' \
      "$smoke_json_stdout" >/dev/null || fail "adoption: scaffolded smoke verb --json did not write run JSON"
    ! grep -q "  result: " "$smoke_json_stderr" \
      || fail "adoption: scaffolded smoke verb --json printed human summary"
    run_json_stdout="$st/run-json.stdout"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" nix run "$project#run" -- --task smoke --timeout-ms 60000 --json ) \
      >"$run_json_stdout" || fail "adoption: scaffolded run control app --json failed"
    jq -e '.task.success == true and (.durationMs >= 0) and (.runSummaryPath | type == "string")' \
      "$run_json_stdout" >/dev/null || fail "adoption: scaffolded run control app --json did not write run JSON"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" nix run "$project#ps" ) \
      >/dev/null || fail "adoption: scaffolded ps failed"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" nix run "$project#down" ) \
      >/dev/null || fail "adoption: scaffolded down failed"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" nix run "$project#clean" ) \
      >/dev/null || fail "adoption: scaffolded clean failed"
    before=$(cat "$project/nixfied.nix")
    nix run "$checkout#upgrade" -- --root "$project" --nixfied-url "$pin" \
      || fail "adoption: upgrade failed"
    after=$(cat "$project/nixfied.nix")
    [ "$before" = "$after" ] || fail "adoption: upgrade modified the project-owned nixfied.nix"
    git -C "$project" add -A
    git -C "$project" commit -q --allow-empty -m upgrade
    model="$(nix build --no-link --print-out-paths "$project#model")/model.json"
    st=$(mktemp -d)
    wk=$(mktemp -d)
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --task smoke --timeout-ms 60000 ) \
      >/dev/null || fail "adoption: post-upgrade run failed"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" clean --model "$model" ) \
      >/dev/null || fail "adoption: post-upgrade clean failed"
    rm -rf "$project_repo"
    printf '  adoption: %ds\n' "$((SECONDS - t0))" >&2

    echo "  positive (upgrade accepts attrset flake input)" >&2
    t0=$SECONDS
    attr_project=$(mktemp -d)
    cat >"$attr_project/flake.nix" <<EOF
{
  description = "attrset upgrade fixture";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    nixfied = {
      url = "github:willyrgf/nixfied";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixfied, nixpkgs }: {};
}
EOF
    nix run "$checkout#upgrade" -- --root "$attr_project" --no-lock \
      >/dev/null || fail "attrset upgrade: existing nixfied attrset input was not accepted"
    nix run "$checkout#upgrade" -- --root "$attr_project" --nixfied-url "$pin" --no-lock \
      >/dev/null || fail "attrset upgrade: repin failed"
    actual_pin=$(
      NIXFIED_GATE_FLAKE="$attr_project/flake.nix" nix eval --raw --impure --expr '
        let
          flake = import (builtins.getEnv "NIXFIED_GATE_FLAKE");
        in
          flake.inputs.nixfied.url
      '
    )
    [ "$actual_pin" = "$pin" ] || fail "attrset upgrade: nixfied url was not rewritten"
    actual_follows=$(
      NIXFIED_GATE_FLAKE="$attr_project/flake.nix" nix eval --raw --impure --expr '
        let
          flake = import (builtins.getEnv "NIXFIED_GATE_FLAKE");
        in
          flake.inputs.nixfied.inputs.nixpkgs.follows
      '
    )
    [ "$actual_follows" = "nixpkgs" ] || fail "attrset upgrade: nixpkgs follows line was not preserved"
    rm -rf "$attr_project"
    printf '  attrset_upgrade: %ds\n' "$((SECONDS - t0))" >&2
  '';
}
