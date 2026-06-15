# The framework gate coordinator: runs nix-layer tests (gate-nix) then the
# runtime-layer tests (examples, slots, negatives, lifecycle). There is no
# self-model and no orchestrator binary.
#
# `nix run .#gate` builds the debug runtime + the example models from the
# working tree and runs everything against a fixed state dir
# ($TMPDIR/nixfied-gate), wiped fresh each run and kept afterward.
{
  pkgs,
  gateNix,
  runtime,
  models,
}:
pkgs.writeShellApplication {
  name = "nixfied-gate";
  runtimeInputs = [
    pkgs.jq
    pkgs.coreutils
    pkgs.diffutils
    pkgs.sqlite
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

    # `--dirty` (or NIXFIED_GATE_DIRTY=1) pins the adoption test to the working
    # tree (`path:`) instead of HEAD.
    dirty="''${NIXFIED_GATE_DIRTY:-}"
    for arg in "$@"; do
      case "$arg" in
        --dirty) dirty=1 ;;
        *) fail "unknown gate argument: $arg (supported: --dirty)" ;;
      esac
    done

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
      local name="$1" dir="$2" task="$3"
      local model="$dir/model.json"
      echo "  example $name" >&2
      local st="$state/$name-state" wk="$state/$name-work"
      mkdir -p "$st" "$wk"
      local args=(run --model "$model" --task "$task" --timeout-ms 60000)
      ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" "''${args[@]}" ) \
        > "$artifacts/$name.json" 2> "$st/run.err" \
        || fail "$name: run failed — $(tail -n1 "$st/run.err")"
      jq -e '(.services | length > 0) or (.nodes | length > 0)' \
        "$artifacts/$name.json" >/dev/null \
        || fail "$name: run reported no services or nodes"
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
        > "$artifacts/$name.clean.json" || fail "$name: clean failed"
      [ ! -d "$st/$pid/dev/0" ] || fail "$name: clean left the slot state root"
      printf '  %-11s %-44s %s\n' "$name" \
        "$(jq -r '[.services[]? | "\(.serviceId):\(.selectedEndpoint.port)"] | join(" ")' "$artifacts/$name.json")" \
        "$st/$pid/dev/0" >> "$state/summary.txt"
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
      ( cd "$w0" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --task release --slot 0 --timeout-ms 60000 ) \
        > "$artifacts/slots-0.json" 2> "$st/0.err" &
      p0=$!
      ( cd "$w1" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --task release --slot 1 --timeout-ms 60000 ) \
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
        > "$artifacts/slots-0.clean.json" || fail "slots: clean slot 0 failed"
      { [ ! -d "$st/$pid/dev/0" ] && [ -d "$st/$pid/dev/1" ]; } \
        || fail "slots: cleaning slot 0 disturbed slot 1 or left slot 0"
      ( cd "$w1" && NIXFIED_STATE_DIR="$st" "$rt" clean --model "$model" --slot 1 ) \
        > "$artifacts/slots-1.clean.json" || fail "slots: clean slot 1 failed"
      [ ! -d "$st/$pid/dev/1" ] || fail "slots: clean did not remove slot 1"
      jq -n \
        --slurpfile a "$artifacts/slots-0.json" \
        --slurpfile b "$artifacts/slots-1.json" \
        '{check:"slots", status:"pass",
          slots:[{slot:0, services:$a[0].services},
                 {slot:1, services:$b[0].services}]}' \
        > "$artifacts/slots.json"
      printf '  %-11s slot0 %-22s slot1 %s\n' slots \
        "$(jq -r '[.services[].selectedEndpoint.port] | join(",")' "$artifacts/slots-0.json")" \
        "$(jq -r '[.services[].selectedEndpoint.port] | join(",")' "$artifacts/slots-1.json")" \
        >> "$state/summary.txt"
    }

    # Non-nix fail-closed checks: runtime-layer negatives that belong here until
    # gate-runtime is wired in Phase 2/3. The nix-eval checks moved to gate-nix.
    negative() {
      echo "  negative (run with no selection refuses and lists declared tasks)" >&2
      local st="$state/negative-state"
      mkdir -p "$st"
      if ( NIXFIED_STATE_DIR="$st" "$rt" run \
            --model "${models.minimal}/model.json" ) \
            >/dev/null 2>"$artifacts/negative-selection.json"; then
        fail "negative: the runtime accepted a run with no task selection"
      fi
      tail -n 1 "$artifacts/negative-selection.json" \
        | jq -e '.details.declaredTasks | index("smoke") != null' >/dev/null \
        || fail "negative: the selection refusal did not list the declared tasks"

      echo "  negative (an undeclared task selection must be refused)" >&2
      if ( NIXFIED_STATE_DIR="$st" "$rt" run \
            --model "${models.minimal}/model.json" --task does-not-exist ) \
            >/dev/null 2>"$artifacts/negative-unknown-task.json"; then
        fail "negative: the runtime accepted an undeclared task"
      fi

      echo "  negative (run failure must carry run identity and state paths)" >&2
      if ( NIXFIED_STATE_DIR="$st/identity" "$rt" run \
            --model "${models.negativeFail}/model.json" --task failing ) \
            >/dev/null 2>"$artifacts/negative-fail.json"; then
        fail "negative: the failing composite run reported success"
      fi
      tail -n 1 "$artifacts/negative-fail.json" \
        | jq -e '.details.runId and .details.stateRoot and .details.logsDir' >/dev/null \
        || fail "negative: failure JSON is missing runId/stateRoot/logsDir details"
    }

    # The state lifecycle matrix over one shared state dir: second run (adopt),
    # changed model hash (in-place upgrade preserving state), changed state
    # epoch (upgrade with clean), and tampered-marker refusals.
    # Interrupt-recover moved to the cargo test floor (lifecycle.rs).
    lifecycle() {
      echo "  lifecycle (second run: marker adopted)" >&2
      local st="$state/lifecycle-state" root marker hash1 hash2
      mkdir -p "$st"
      root="$st/minimal/dev/0"
      marker="$root/.nixfied-state.json"
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimal}/model.json" --task smoke \
        >/dev/null || fail "lifecycle: first run failed"
      touch "$root/sentinel"
      hash1=$(jq -r .computedModelHash "$marker")
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimal}/model.json" --task smoke \
        >/dev/null || fail "lifecycle: second run of the same model failed"
      [ -e "$root/sentinel" ] || fail "lifecycle: second run lost the state root"
      [ "$(jq -r .computedModelHash "$marker")" = "$hash1" ] \
        || fail "lifecycle: second run rewrote marker provenance"

      echo "  lifecycle (changed model hash: in-place upgrade, state preserved)" >&2
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimalB}/model.json" --task smoke \
        >/dev/null || fail "lifecycle: changed-model run was refused"
      [ -e "$root/sentinel" ] \
        || fail "lifecycle: same-epoch upgrade cleaned the state root"
      hash2=$(jq -r .computedModelHash "$marker")
      [ "$hash2" != "$hash1" ] || fail "lifecycle: upgrade did not rewrite provenance"
      upgrades=$(sqlite3 "$st/registry/minimal/dev/0/registry.sqlite3" \
        "SELECT count(*) FROM events WHERE event_type = 'state.upgraded'")
      [ "$upgrades" -ge 1 ] || fail "lifecycle: upgrade left no state.upgraded event"

      echo "  lifecycle (changed state epoch: upgrade with clean)" >&2
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimalEpoch2}/model.json" --task smoke \
        >/dev/null || fail "lifecycle: epoch-change run was refused"
      [ ! -e "$root/sentinel" ] \
        || fail "lifecycle: epoch upgrade preserved state across the declared boundary"
      [ "$(jq -r .stateEpoch "$marker")" = "2" ] \
        || fail "lifecycle: epoch upgrade did not record the new epoch"

      echo "  lifecycle (tampered marker: ownership and ABI refusals)" >&2
      local code
      jq '.projectId = "intruder"' "$marker" > "$marker.tmp" && mv "$marker.tmp" "$marker"
      code=0
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimalEpoch2}/model.json" --task smoke \
        >/dev/null 2>"$artifacts/lifecycle-tamper-owner.json" || code=$?
      [ "$code" -eq 21 ] \
        || fail "lifecycle: tampered ownership exited $code, want 21 (STATE_UNOWNED)"
      jq '.projectId = "minimal" | .runtimeAbi = "nixfied-runtime-abi:0-foreign"' \
        "$marker" > "$marker.tmp" && mv "$marker.tmp" "$marker"
      code=0
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimalEpoch2}/model.json" --task smoke \
        >/dev/null 2>"$artifacts/lifecycle-tamper-abi.json" || code=$?
      [ "$code" -eq 21 ] \
        || fail "lifecycle: tampered runtime ABI exited $code, want 21 (STATE_UNOWNED)"
      printf '  %-11s %s\n' lifecycle \
        "adopt -> upgrade(preserve) -> upgrade(epoch clean) -> tamper refused" \
        >> "$state/summary.txt"
    }

    echo "==> gate-nix" >&2
    NIXFIED_GATE_CHECKOUT="$checkout" NIXFIED_GATE_DIRTY="$dirty" NIXFIED_GATE_STATE="$state" \
      ${gateNix}/bin/nixfied-gate-nix
    echo "==> examples (run + views + clean)" >&2
    example minimal "${models.minimal}" smoke
    example postgres "${models.postgres}" smoke-query
    example composite "${models.composite}" pipeline
    example polyglot "${models.polyglot}" all
    example downstream "${models.downstream}" release
    example reth "${models.reth}" reth-smoke
    example toolchain "${models.toolchain}" ci
    echo "==> slots" >&2
    slots
    echo "==> negative" >&2
    negative
    echo "==> lifecycle" >&2
    lifecycle
    echo "  gate: all checks passed" >&2
    echo "" >&2
    echo "  execution detail (service:port + state root per check; full JSON in $artifacts):" >&2
    cat "$state/summary.txt" >&2
  '';
}
