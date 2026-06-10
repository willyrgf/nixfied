# The framework gate: exercise the runtime the way adopters do — run the example
# models as ordinary top-level runs — plus the few checks a single run can't make
# on its own (cross-slot isolation, fail-closed, the adoption loop). There is no
# self-model and no orchestrator binary: each example is its own spec (it fails if
# its services/tasks fail), and the only bespoke logic is the cross-run `slots`
# assertion.
#
# `nix run .#gate` builds the debug runtime + the example models from the working
# tree and runs everything against a fixed state dir ($TMPDIR/nixfied-gate), wiped
# fresh each run and kept afterward so the per-check artifacts stay inspectable.
{
  pkgs,
  runtime,
  models,
}:
pkgs.writeShellApplication {
  name = "nixfied-gate";
  runtimeInputs = [
    pkgs.nix
    pkgs.git
    pkgs.jq
    pkgs.coreutils
    pkgs.diffutils
  ];
  text = ''
    rt="${runtime}/bin/nixfied-runtime"
    cli="${runtime}/bin/nixfied"
    checkout="''${NIXFIED_GATE_CHECKOUT:-$PWD}"

    state="''${TMPDIR:-/tmp}/nixfied-gate"
    rm -rf "$state"
    mkdir -p "$state"
    artifacts="$state/artifacts"
    mkdir -p "$artifacts"
    echo "    state + artifacts: $state" >&2

    fail() {
      echo "  GATE FAIL: $*" >&2
      exit 1
    }

    # `slots-0.json` vs `slots-1.json`: is the given jq array expression disjoint?
    disjoint() {
      local inter
      inter=$(jq -n \
        --argjson a "$(jq "$1" "$artifacts/slots-0.json")" \
        --argjson b "$(jq "$1" "$artifacts/slots-1.json")" \
        '[ $a[] | select( . as $x | $b | index($x) ) ] | length')
      [ "$inter" -eq 0 ]
    }

    # Run one example as an adopter would, verify its emitted views project from
    # the model (nix-emitted == runtime-rederived), then clean.
    example() {
      local name="$1" dir="$2" workflow="''${3:-}"
      local model="$dir/model.json"
      echo "  example $name" >&2
      local st="$state/$name-state" wk="$state/$name-work"
      mkdir -p "$st" "$wk"
      local args=(run --model "$model" --timeout-ms 60000)
      [ -n "$workflow" ] && args+=(--workflow "$workflow")
      ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" "''${args[@]}" ) \
        > "$artifacts/$name.json" 2> "$st/run.err" \
        || fail "$name: run failed — $(tail -n1 "$st/run.err")"
      jq -e '(.services | length > 0) or (.workflowNodes | length > 0)' \
        "$artifacts/$name.json" >/dev/null \
        || fail "$name: run reported no services or workflow nodes"
      # The runtime re-derives each view from model.json; it must project the same
      # view nix emitted. Compare JSON semantically (formatting is not contract);
      # docs is text.
      local sub
      for sub in schema capabilities; do
        diff <("$cli" "$sub" --model "$model" | jq -S .) <(jq -S . "$dir/views/$sub.json") \
          >/dev/null || fail "$name: $sub view drifted between nix and the runtime"
      done
      diff <("$cli" docs --model "$model") "$dir/views/docs.md" >/dev/null \
        || fail "$name: docs view drifted between nix and the runtime"
      local pid
      pid=$(jq -r '.project.projectId' "$model")
      ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" clean --model "$model" ) \
        || fail "$name: clean failed"
      [ ! -d "$st/$pid/dev/0" ] || fail "$name: clean left the slot state root"
    }

    # The one check a single run can't make: two slots of a multi-service model
    # run concurrently against one state base must stay fully isolated.
    slots() {
      echo "  slots (two concurrent slots of downstream)" >&2
      local model="${models.downstream}/model.json"
      local pid st w0 w1 p0 p1
      pid=$(jq -r '.project.projectId' "$model")
      st="$state/slots-state"
      w0="$state/slots-w0"
      w1="$state/slots-w1"
      mkdir -p "$st" "$w0" "$w1"
      ( cd "$w0" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --slot 0 --timeout-ms 60000 ) \
        > "$artifacts/slots-0.json" 2> "$st/0.err" &
      p0=$!
      ( cd "$w1" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --slot 1 --timeout-ms 60000 ) \
        > "$artifacts/slots-1.json" 2> "$st/1.err" &
      p1=$!
      wait "$p0" || fail "slots: slot 0 run failed — $(tail -n1 "$st/0.err")"
      wait "$p1" || fail "slots: slot 1 run failed — $(tail -n1 "$st/1.err")"
      disjoint '[.services[].selectedEndpoint.port]' || fail "slots: ports overlapped"
      disjoint '[.services[].serviceInstanceId]' || fail "slots: shared a serviceInstanceId"
      disjoint '[.services[].processKey]' || fail "slots: shared a processKey (same OS process)"
      { [ -f "$st/$pid/dev/0/pgdata/PG_VERSION" ] && [ -f "$st/$pid/dev/1/pgdata/PG_VERSION" ]; } \
        || fail "slots: each slot must own a separate Postgres data cluster"
      ( cd "$w0" && NIXFIED_STATE_DIR="$st" "$rt" clean --model "$model" --slot 0 ) \
        || fail "slots: clean slot 0 failed"
      { [ ! -d "$st/$pid/dev/0" ] && [ -d "$st/$pid/dev/1" ]; } \
        || fail "slots: cleaning slot 0 disturbed slot 1 or left slot 0"
      ( cd "$w1" && NIXFIED_STATE_DIR="$st" "$rt" clean --model "$model" --slot 1 ) \
        || fail "slots: clean slot 1 failed"
      [ ! -d "$st/$pid/dev/1" ] || fail "slots: clean did not remove slot 1"
      jq -n \
        --slurpfile a "$artifacts/slots-0.json" \
        --slurpfile b "$artifacts/slots-1.json" \
        '{check:"slots", status:"pass",
          slots:[{slot:0, services:$a[0].services},
                 {slot:1, services:$b[0].services}]}' \
        > "$artifacts/slots.json"
    }

    # Fail-closed: selecting an undeclared workflow must be refused.
    negative() {
      echo "  negative (undeclared workflow must be refused)" >&2
      local st="$state/negative-state"
      mkdir -p "$st"
      if ( NIXFIED_STATE_DIR="$st" "$rt" run \
            --model "${models.minimal}/model.json" --workflow does-not-exist ) \
            >/dev/null 2>&1; then
        fail "negative: the runtime accepted an undeclared workflow"
      fi
    }

    # The real adoption loop: scaffold a throwaway repo pinning the checkout, build
    # + run + clean, then upgrade (must not touch the project-owned nixfied.nix),
    # rebuild + run + clean.
    adoption() {
      echo "  adoption (#install + #upgrade against a throwaway repo)" >&2
      local pin="path:$checkout" project st wk model before after
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
      ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --timeout-ms 60000 ) \
        >/dev/null || fail "adoption: scaffolded run failed"
      ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" clean --model "$model" ) \
        || fail "adoption: scaffolded clean failed"
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
      ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --timeout-ms 60000 ) \
        >/dev/null || fail "adoption: post-upgrade run failed"
      ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" clean --model "$model" ) \
        || fail "adoption: post-upgrade clean failed"
      rm -rf "$project"
    }

    echo "==> examples (run + views + clean)" >&2
    example minimal "${models.minimal}"
    example postgres "${models.postgres}"
    example workflow "${models.workflow}" pipeline
    example polyglot "${models.polyglot}"
    example downstream "${models.downstream}" release
    echo "==> slots" >&2
    slots
    echo "==> negative" >&2
    negative
    echo "==> adoption" >&2
    adoption
    echo "  gate: all checks passed" >&2
  '';
}
