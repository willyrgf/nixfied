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
    # tree (`path:`) instead of HEAD. A `path:` pin hashes the whole directory —
    # `.git/` included — so every run re-derives the entire closure; the default
    # rev pin keeps the store cache warm but only exercises committed code.
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

    # Fail-closed: selecting an undeclared workflow must be refused, and a workflow
    # authored with a duplicate node id must not even compile (workflow `nodes` is
    # an attrset keyed by node id, so a duplicate is a Nix evaluation error rather
    # than a silent last-wins collapse).
    negative() {
      echo "  negative (undeclared workflow must be refused)" >&2
      local st="$state/negative-state"
      mkdir -p "$st"
      if ( NIXFIED_STATE_DIR="$st" "$rt" run \
            --model "${models.minimal}/model.json" --workflow does-not-exist ) \
            >/dev/null 2>"$artifacts/negative-error.json"; then
        fail "negative: the runtime accepted an undeclared workflow"
      fi

      echo "  negative (run failure must carry run identity and state paths)" >&2
      # The error JSON is the final stderr line; progress/warning lines precede it.
      tail -n 1 "$artifacts/negative-error.json" \
        | jq -e '.details.runId and .details.stateRoot and .details.logsDir' >/dev/null \
        || fail "negative: failure JSON is missing runId/stateRoot/logsDir details"

      echo "  negative (duplicate workflow node id must not compile)" >&2
      if nix eval --expr \
            '{ nodes = { dup = { taskId = "a"; }; dup = { taskId = "b"; }; }; }' \
            >/dev/null 2>&1; then
        fail "negative: a duplicate workflow node id evaluated successfully"
      fi
    }

    # The state lifecycle matrix over one shared state dir: second run (adopt),
    # changed model hash (in-place upgrade preserving state), changed state
    # epoch (upgrade with clean), an interrupted run recovered by the next
    # model's run (live old-model service torn down through the registry), and
    # tampered-marker refusals. This is the repeatability story the first
    # adopter's review demanded proven end-to-end.
    lifecycle() {
      echo "  lifecycle (second run: marker adopted)" >&2
      local st="$state/lifecycle-state" root marker hash1 hash2
      mkdir -p "$st"
      root="$st/minimal/dev/0"
      marker="$root/.nixfied-state.json"
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimal}/model.json" \
        >/dev/null || fail "lifecycle: first run failed"
      touch "$root/sentinel"
      hash1=$(jq -r .computedModelHash "$marker")
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimal}/model.json" \
        >/dev/null || fail "lifecycle: second run of the same model failed"
      [ -e "$root/sentinel" ] || fail "lifecycle: second run lost the state root"
      [ "$(jq -r .computedModelHash "$marker")" = "$hash1" ] \
        || fail "lifecycle: second run rewrote marker provenance"

      echo "  lifecycle (changed model hash: in-place upgrade, state preserved)" >&2
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimalB}/model.json" \
        >/dev/null || fail "lifecycle: changed-model run was refused"
      [ -e "$root/sentinel" ] \
        || fail "lifecycle: same-epoch upgrade cleaned the state root"
      hash2=$(jq -r .computedModelHash "$marker")
      [ "$hash2" != "$hash1" ] || fail "lifecycle: upgrade did not rewrite provenance"
      upgrades=$(sqlite3 "$st/registry/minimal/dev/0/registry.sqlite3" \
        "SELECT count(*) FROM events WHERE event_type = 'state.upgraded'")
      [ "$upgrades" -ge 1 ] || fail "lifecycle: upgrade left no state.upgraded event"

      echo "  lifecycle (changed state epoch: upgrade with clean)" >&2
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimalEpoch2}/model.json" \
        >/dev/null || fail "lifecycle: epoch-change run was refused"
      [ ! -e "$root/sentinel" ] \
        || fail "lifecycle: epoch upgrade preserved state across the declared boundary"
      [ "$(jq -r .stateEpoch "$marker")" = "2" ] \
        || fail "lifecycle: epoch upgrade did not record the new epoch"

      echo "  lifecycle (interrupted run: live old-model service recovered)" >&2
      local pst="$state/lifecycle-interrupt-state" ppid live
      mkdir -p "$pst"
      NIXFIED_STATE_DIR="$pst" "$rt" run --model "${models.postgresSlow}/model.json" \
        >"$artifacts/lifecycle-interrupt.json" 2>&1 &
      ppid=$!
      for _ in $(seq 1 300); do
        if (echo > /dev/tcp/127.0.0.1/24580) 2>/dev/null; then break; fi
        sleep 0.2
      done
      (echo > /dev/tcp/127.0.0.1/24580) 2>/dev/null \
        || fail "lifecycle: interrupted-run postgres never came up"
      kill -9 "$ppid" 2>/dev/null || true
      wait "$ppid" 2>/dev/null || true
      live=$(NIXFIED_STATE_DIR="$pst" "$rt" ps --model "${models.postgresSlow}/model.json" \
        | jq '[.processes[] | select(.live)] | length')
      [ "$live" -ge 1 ] || fail "lifecycle: interrupted run left no live service to recover"
      # The next model's run must recover on its own: reconcile the dead
      # runtime's evidence, stop the orphaned postgres, adopt pgdata (same
      # epoch + idempotent prepare), and proceed.
      NIXFIED_STATE_DIR="$pst" "$rt" run --model "${models.postgres}/model.json" \
        >/dev/null || fail "lifecycle: recovery run after interrupt failed"
      [ -s "$pst/postgres-example/dev/0/pgdata/PG_VERSION" ] \
        || fail "lifecycle: recovery run did not adopt the existing cluster"
      NIXFIED_STATE_DIR="$pst" "$rt" clean --model "${models.postgres}/model.json" \
        >/dev/null || fail "lifecycle: clean after recovery failed"
      [ ! -e "$pst/postgres-example/dev/0" ] || fail "lifecycle: clean left the state root"

      echo "  lifecycle (tampered marker: ownership and ABI refusals)" >&2
      local code
      jq '.projectId = "intruder"' "$marker" > "$marker.tmp" && mv "$marker.tmp" "$marker"
      code=0
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimalEpoch2}/model.json" \
        >/dev/null 2>"$artifacts/lifecycle-tamper-owner.json" || code=$?
      [ "$code" -eq 21 ] \
        || fail "lifecycle: tampered ownership exited $code, want 21 (STATE_UNOWNED)"
      jq '.projectId = "minimal" | .runtimeAbi = "nixfied-runtime-abi:0-foreign"' \
        "$marker" > "$marker.tmp" && mv "$marker.tmp" "$marker"
      code=0
      NIXFIED_STATE_DIR="$st" "$rt" run --model "${models.minimalEpoch2}/model.json" \
        >/dev/null 2>"$artifacts/lifecycle-tamper-abi.json" || code=$?
      [ "$code" -eq 21 ] \
        || fail "lifecycle: tampered runtime ABI exited $code, want 21 (STATE_UNOWNED)"
      printf '  %-11s %s\n' lifecycle \
        "adopt -> upgrade(preserve) -> upgrade(epoch clean) -> interrupt+recover -> tamper refused" \
        >> "$state/summary.txt"
    }

    # The real adoption loop: scaffold a throwaway repo pinning the checkout, build
    # + run + clean, then upgrade (must not touch the project-owned nixfied.nix),
    # rebuild + run + clean.
    adoption() {
      echo "  adoption (#install + #upgrade against a throwaway repo)" >&2
      local pin project st wk model before after
      if [ -n "$dirty" ]; then
        pin="path:$checkout"
        echo "    pin: $pin (--dirty: every run re-derives the closure)" >&2
      else
        pin="git+file://$checkout?rev=$(git -C "$checkout" rev-parse HEAD)"
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
      ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --timeout-ms 60000 ) \
        >/dev/null || fail "adoption: scaffolded run failed"
      # The generated control surface: ps/down/clean must exist as project apps
      # and work against the same state.
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
      ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" run --model "$model" --timeout-ms 60000 ) \
        >/dev/null || fail "adoption: post-upgrade run failed"
      ( cd "$wk" && NIXFIED_STATE_DIR="$st" "$rt" clean --model "$model" ) \
        >/dev/null || fail "adoption: post-upgrade clean failed"
      printf '  %-11s %s\n' adoption "install -> build -> run -> clean -> upgrade -> rebuild -> run -> clean" \
        >> "$state/summary.txt"
      rm -rf "$project"
    }

    echo "==> examples (run + views + clean)" >&2
    example minimal "${models.minimal}"
    example postgres "${models.postgres}"
    example workflow "${models.workflow}" pipeline
    example polyglot "${models.polyglot}"
    example downstream "${models.downstream}" release
    example reth "${models.reth}"
    echo "==> slots" >&2
    slots
    echo "==> negative" >&2
    negative
    echo "==> lifecycle" >&2
    lifecycle
    echo "==> adoption" >&2
    adoption
    echo "  gate: all checks passed" >&2
    echo "" >&2
    echo "  execution detail (service:port + state root per check; full JSON in $artifacts):" >&2
    cat "$state/summary.txt" >&2
  '';
}
