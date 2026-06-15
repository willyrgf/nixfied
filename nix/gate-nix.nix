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
    # example and must fail to build.
    reject_composite() {
      if nix build --no-link --impure --expr \
          "let flake = builtins.getFlake (toString $checkout); compileModel = (builtins.getAttr builtins.currentSystem flake.lib).compileModel; in compileModel ({ lib, ... }: { imports = [ $checkout/examples/composite/nixfied.nix ]; $2 })" \
          >/dev/null 2>&1; then
        fail "negative: $1 compiled instead of failing at evaluation"
      fi
    }

    t0=$SECONDS
    echo "  negative (duplicate step name must not compile)" >&2
    if nix eval --expr \
          '{ steps = { dup = { task = "a"; }; dup = { task = "b"; }; }; }' \
          >/dev/null 2>&1; then
      fail "negative: a duplicate step name evaluated successfully"
    fi

    echo "  negative (invalid composites must fail at nix evaluation)" >&2
    reject_composite "an undeclared step task" \
      'nixfied.tasks.pipeline.steps.bad.task = "missing-task";'
    reject_composite "dependsOn an unknown step" \
      'nixfied.tasks.pipeline.steps.verify.dependsOn = lib.mkForce [ "ghost-step" ];'
    reject_composite "an empty composite" \
      'nixfied.tasks.pipeline.steps = lib.mkForce { };'
    reject_composite "a cyclic step graph" \
      'nixfied.tasks.pipeline.steps.probe.dependsOn = lib.mkForce [ "verify" ];'

    echo "  negative (phase 1-5 rules must fail at nix evaluation)" >&2
    reject_composite "a runtime-owned PATH declared in env" \
      'nixfied.tasks.smoke.invocation.env.PATH = "/usr/bin";'
    reject_composite "an unresolvable run[0]" \
      'nixfied.tasks.smoke.invocation.run = lib.mkForce [ "ghost-program" ];'
    reject_composite "a dotted task id (step-path discipline)" \
      'nixfied.closures.synthetic-helper.operationBindings = lib.mkForce null; nixfied.tasks."has.dot" = { invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; }; };'
    reject_composite "a leaf carrying steps" \
      'nixfied.tasks.smoke.steps.bad.task = "smoke";'
    reject_composite "a composite carrying an invocation" \
      'nixfied.tasks.pipeline.invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" ]; };'
    reject_composite "a cyclic task reference graph" \
      'nixfied.tasks.loop-a = { kind = "composite"; steps.next.task = "loop-b"; }; nixfied.tasks.loop-b = { kind = "composite"; steps.next.task = "loop-a"; };'
    reject_composite "an operation binding gate narrower than the derivation" \
      'nixfied.closures.synthetic-helper.operationBindings = lib.mkForce [ "task.smoke.run" ];'
    reject_composite "a duplicate effective operation id" \
      'nixfied.tasks.smoke.operationId = "service.synthetic.start";'
    # shellcheck disable=SC2016
    reject_composite "a task named endpoint ref outside requires" \
      'nixfied.tasks.smoke.invocation.run = lib.mkForce [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "\''${port:ghost}" ];'
    # shellcheck disable=SC2016
    reject_composite "a service named endpoint ref outside connectsTo" \
      'nixfied.services.synthetic.lifecycle.start.invocation.run = lib.mkForce [ "nixfied-synthetic-helper" "service" "--host" "127.0.0.1" "--port" "\''${port:ghost}" ];'
    reject_composite "both endpoint forms set" \
      'nixfied.services.synthetic.endpoints = { extra = { }; }; nixfied.services.synthetic.primaryEndpoint = "extra";'
    reject_composite "a tcp probe on an endpoint-less service" \
      'nixfied.services.bare = { lifecycle.start.invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" "service" "--host" "127.0.0.1" "--port" "1" ]; }; };'
    reject_composite "a listening service without the network-listener attestation" \
      'nixfied.closures.synthetic-helper.effects = lib.mkForce [ "process" ];'
    reject_composite "an endpoint-less lifecycle using a bare endpoint placeholder" \
      'nixfied.closures.synthetic-helper.effects = lib.mkForce [ "process" ]; nixfied.services.synthetic.endpoint = lib.mkForce null; nixfied.services.synthetic.lifecycle.ready.probe = { kind = "exec"; invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; }; }; nixfied.services.synthetic.lifecycle.health.probe = { kind = "exec"; invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; }; };'
    # shellcheck disable=SC2016
    reject_composite "a task addressing an endpoint-less required service" \
      'nixfied.closures.synthetic-helper.effects = lib.mkForce [ "process" ]; nixfied.services.synthetic.endpoint = lib.mkForce null; nixfied.services.synthetic.lifecycle.start.invocation.run = lib.mkForce [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; nixfied.services.synthetic.lifecycle.ready.probe = { kind = "exec"; invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; }; }; nixfied.services.synthetic.lifecycle.health.probe = { kind = "exec"; invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; }; }; nixfied.tasks.smoke.invocation.run = lib.mkForce [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "\''${port:synthetic}" ];'
    reject_composite "an endpoint-less service start declaring network-listener" \
      'nixfied.services.synthetic.endpoint = lib.mkForce null; nixfied.services.synthetic.lifecycle.start.invocation.run = lib.mkForce [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; nixfied.services.synthetic.lifecycle.ready.probe = { kind = "exec"; invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; }; }; nixfied.services.synthetic.lifecycle.health.probe = { kind = "exec"; invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; }; };'
    reject_composite "a dangling prepare task" \
      'nixfied.services.synthetic.lifecycle.prepare.task = "ghost";'
    reject_composite "a prepare requiring its own service (combined-graph cycle)" \
      'nixfied.closures.synthetic-helper.operationBindings = lib.mkForce null; nixfied.tasks.selfinit = { invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; }; requires = [ "synthetic" ]; }; nixfied.services.synthetic.lifecycle.prepare.task = "selfinit";'
    reject_composite "a dangling surface verb" \
      'nixfied.surface.verbs = [ "ghost" ];'
    reject_composite "a surface verb colliding with the control namespace" \
      'nixfied.closures.synthetic-helper.operationBindings = lib.mkForce null; nixfied.tasks.clean = { invocation = { tools = [ "synthetic-helper" ]; run = [ "nixfied-synthetic-helper" "task" "--host" "127.0.0.1" "--port" "1" ]; }; }; nixfied.surface.verbs = [ "clean" ];'
    printf '  reject_composite: %ds\n' "$((SECONDS - t0))" >&2

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
    # The generated control surface: ps/down/clean must exist as project apps
    # and work against the same state.
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" nix run "$project#admit" ) \
      >/dev/null || fail "adoption: scaffolded admit failed"
    ( cd "$wk" && NIXFIED_STATE_DIR="$st" nix run "$project#smoke" -- --timeout-ms 60000 ) \
      >/dev/null || fail "adoption: scaffolded smoke verb failed"
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
