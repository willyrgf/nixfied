# Nix-layer tests: checks that exercise the Nix compiler and install tooling.
# These stay in bash because expressing them as runtime tasks would create tasks
# that do arbitrary builds and network I/O — a violation of the bounded-execution
# semantics of a task and a blurring of the framework's layer boundary.
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

    # Composite structural validation is the Nix layer's job: a broken or
    # cyclic composite must throw at evaluation, never compile into a model
    # the runtime only rejects later. Each case overrides the valid composite
    # example and must fail while evaluating the model derivation.
    reject_composites() {
      local unexpected
      if ! unexpected=$(nix eval --json --impure --expr "import ${./gate-nix-negatives.nix} { checkout = $checkout; }"); then
        fail "negative: batched invalid composite evaluation failed"
      fi
      if [ "$unexpected" != "[]" ]; then
        echo "  unexpected successful negative cases:" >&2
        echo "$unexpected" | jq -r '.[] | "    - " + .' >&2
        fail "negative: invalid composites compiled instead of failing at evaluation"
      fi
    }

    t0=$SECONDS
    echo "  negative (invalid composites must fail at nix evaluation)" >&2
    echo "  negative (phase 1-5 rules must fail at nix evaluation)" >&2
    reject_composites
    printf '  reject_composites: %ds\n' "$((SECONDS - t0))" >&2

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
    project=$(mktemp -d)
    git -C "$project" init -q
    git -C "$project" config user.email gate@nixfied
    git -C "$project" config user.name "nixfied gate"
    nix run "$checkout#install" -- \
      --root "$project" --project-id adopt --name adopt --nixfied-url "$pin" \
      || fail "adoption: install failed"
    git -C "$project" add -A
    git -C "$project" commit -q -m scaffold
    if nix run "$checkout#install" -- --root "$project" >/dev/null 2>&1; then
      fail "adoption: re-running install did not refuse an existing flake.nix"
    fi
    model="$(nix build --no-link --print-out-paths "$project#model")/model.json"
    st=$(mktemp -d)
    wk=$(mktemp -d)
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --task smoke --timeout-ms 60000 ) \
      >/dev/null || fail "adoption: scaffolded run failed"
    # The generated control surface: model-check/ps/down/clean must exist as project apps
    # and work against the same state.
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
    git -C "$project" commit -q -m upgrade
    model="$(nix build --no-link --print-out-paths "$project#model")/model.json"
    st=$(mktemp -d)
    wk=$(mktemp -d)
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --task smoke --timeout-ms 60000 ) \
      >/dev/null || fail "adoption: post-upgrade run failed"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" clean --model "$model" ) \
      >/dev/null || fail "adoption: post-upgrade clean failed"
    rm -rf "$project"
    printf '  adoption: %ds\n' "$((SECONDS - t0))" >&2
  '';
}
