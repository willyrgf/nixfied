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
    pkgs.gnutar
    pkgs.gzip
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
    upgrade_transaction_tests() {
      local project model_project lock_failure_project no_lock_project
      local before_flake before_lock before_project bad_pin no_lock_url
      local state

      echo "  upgrade transaction and mode semantics" >&2
      project=$(mktemp -d)
      nix run "$checkout#install" -- --root "$project" --project-id upgrade-plan --name upgrade-plan --nixfied-url "$pin" >/dev/null || fail "upgrade transaction: fixture install failed"
      nix flake lock "$project" >/dev/null || fail "upgrade transaction: fixture lock failed"
      state="$project/runtime-state"
      before_flake=$(sha256sum "$project/flake.nix")
      before_lock=$(sha256sum "$project/flake.lock")
      before_project=$(sha256sum "$project/nixfied.nix")
      nix_plan_stdout="$project/plan.stdout"
      nix_plan_stderr="$project/plan.stderr"
      ( NIXFIED_STATE_DIR="$state" nix run "$checkout#upgrade" -- --root "$project" --nixfied-url "$pin" --plan ) >"$nix_plan_stdout" 2>"$nix_plan_stderr" || fail "upgrade transaction: --plan failed"
      grep -Fq "model preflight: passed" "$nix_plan_stderr" || fail "upgrade transaction: --plan omitted model preflight"
      grep -Fq "plan: no project files changed" "$nix_plan_stderr" || fail "upgrade transaction: --plan omitted no-mutation status"
      [ "$before_flake" = "$(sha256sum "$project/flake.nix")" ] || fail "upgrade transaction: --plan changed flake.nix"
      [ "$before_lock" = "$(sha256sum "$project/flake.lock")" ] || fail "upgrade transaction: --plan changed flake.lock"
      [ "$before_project" = "$(sha256sum "$project/nixfied.nix")" ] || fail "upgrade transaction: --plan changed nixfied.nix"
      [ ! -e "$state" ] || fail "upgrade transaction: --plan materialized runtime state"
      rm -rf "$project"

      missing_lock_project=$(mktemp -d)
      nix run "$checkout#install" -- --root "$missing_lock_project" --project-id upgrade-missing-lock --name upgrade-missing-lock --nixfied-url "$pin" >/dev/null \
        || fail "upgrade transaction: missing-lock fixture install failed"
      [ ! -e "$missing_lock_project/flake.lock" ] || fail "upgrade transaction: missing-lock fixture unexpectedly had a lock"
      before_flake=$(sha256sum "$missing_lock_project/flake.nix")
      before_project=$(sha256sum "$missing_lock_project/nixfied.nix")
      if nix run "$checkout#upgrade" -- --root "$missing_lock_project" --nixfied-url "$pin" >"$missing_lock_project/stdout" 2>"$missing_lock_project/stderr"; then
        fail "upgrade transaction: missing lock unexpectedly entered checked mode"
      fi
      grep -Fq "flake.lock is required for a checked upgrade" "$missing_lock_project/stderr" \
        || fail "upgrade transaction: missing lock refusal was not reported"
      [ "$before_flake" = "$(sha256sum "$missing_lock_project/flake.nix")" ] \
        || fail "upgrade transaction: missing lock changed flake.nix"
      [ "$before_project" = "$(sha256sum "$missing_lock_project/nixfied.nix")" ] \
        || fail "upgrade transaction: missing lock changed nixfied.nix"
      rm -rf "$missing_lock_project"

      project=$(mktemp -d)
      if [ -n "$dirty" ]; then
        race_candidate_pin="git+file://$checkout?rev=$(git -C "$checkout" rev-parse HEAD)&shallow=1"
      else
        race_candidate_pin="path:$checkout"
      fi
      nix run "$checkout#install" -- --root "$project" --project-id upgrade-race --name upgrade-race --nixfied-url "$pin" >/dev/null \
        || fail "upgrade transaction: race fixture install failed"
      nix flake lock "$project" >/dev/null || fail "upgrade transaction: race fixture lock failed"
      before_flake=$(sha256sum "$project/flake.nix")
      before_lock=$(sha256sum "$project/flake.lock")
      before_project=$(sha256sum "$project/nixfied.nix")
      race_state="$project/runtime-state"
      nix run "$checkout#upgrade" -- --root "$project" --nixfied-url "$race_candidate_pin" >"$project/race-one.stdout" 2>"$project/race-one.stderr" &
      race_one=$!
      nix run "$checkout#upgrade" -- --root "$project" --nixfied-url "$race_candidate_pin" >"$project/race-two.stdout" 2>"$project/race-two.stderr" &
      race_two=$!
      race_one_status=0
      race_two_status=0
      wait "$race_one" || race_one_status=$?
      wait "$race_two" || race_two_status=$?
      [ "$race_one_status" -eq 0 ] || [ "$race_two_status" -eq 0 ] \
        || fail "upgrade transaction: concurrent upgrades both failed"
      [ "$race_one_status" -eq 6 ] || [ "$race_two_status" -eq 6 ] \
        || fail "upgrade transaction: concurrent upgrade did not refuse the stale candidate"
      grep -hEq "changed concurrently|concurrently" "$project/race-one.stderr" "$project/race-two.stderr" \
        || fail "upgrade transaction: concurrent upgrade omitted the conflict diagnostic"
      [ "$before_project" = "$(sha256sum "$project/nixfied.nix")" ] \
        || fail "upgrade transaction: concurrent upgrade changed nixfied.nix"
      [ "$before_flake" != "$(sha256sum "$project/flake.nix")" ] \
        || fail "upgrade transaction: concurrent upgrade did not apply flake.nix"
      [ "$before_lock" != "$(sha256sum "$project/flake.lock")" ] \
        || fail "upgrade transaction: concurrent upgrade did not apply flake.lock"
      grep -Fq "$race_candidate_pin" "$project/flake.nix" \
        || fail "upgrade transaction: concurrent upgrade applied the wrong flake pin"
      if [ -n "$dirty" ]; then
        grep -Fq '"type":"git"' "$project/race-one.stderr" "$project/race-two.stderr" \
          || fail "upgrade transaction: concurrent Git candidate identity was not reported"
      else
        grep -Fq '"type":"path"' "$project/race-one.stderr" "$project/race-two.stderr" \
          || fail "upgrade transaction: concurrent path candidate identity was not reported"
      fi
      [ -z "$(find "$project" -maxdepth 1 -type f -name '*nixfied-upgrade*' -print -quit)" ] \
        || fail "upgrade transaction: concurrent upgrade left temporary files"
      [ ! -e "$race_state" ] || fail "upgrade transaction: concurrent upgrade materialized runtime state"
      rm -rf "$project"

      interrupt_project=$(mktemp -d)
      nix run "$checkout#install" -- --root "$interrupt_project" --project-id upgrade-interrupt --name upgrade-interrupt --nixfied-url "$pin" >/dev/null \
        || fail "upgrade transaction: interrupt fixture install failed"
      nix flake lock "$interrupt_project" >/dev/null || fail "upgrade transaction: interrupt fixture lock failed"
      before_flake=$(sha256sum "$interrupt_project/flake.nix")
      before_lock=$(sha256sum "$interrupt_project/flake.lock")
      before_project=$(sha256sum "$interrupt_project/nixfied.nix")
      NIXFIED_UPGRADE_TEST_PAUSE_AFTER_LOCK=30 nix run "$checkout#upgrade" -- --root "$interrupt_project" --nixfied-url "$race_candidate_pin" >"$interrupt_project/stdout" 2>"$interrupt_project/stderr" &
      interrupt_pid=$!
      paused=0
      for attempt in $(seq 1 120); do
        : "$attempt"
        if grep -Fq "test pause after lock apply" "$interrupt_project/stderr"; then
          paused=1
          break
        fi
        kill -0 "$interrupt_pid" 2>/dev/null || break
        sleep 0.25
      done
      [ "$paused" -eq 1 ] || fail "upgrade transaction: interrupt fixture never reached the guarded apply window"
      kill -TERM "$interrupt_pid"
      interrupt_status=0
      wait "$interrupt_pid" || interrupt_status=$?
      [ "$interrupt_status" -ne 0 ] || fail "upgrade transaction: interrupted apply unexpectedly succeeded"
      grep -Fq "upgrade interrupted; candidate rolled back" "$interrupt_project/stderr" \
        || fail "upgrade transaction: interrupted apply omitted rollback status"
      [ "$before_flake" = "$(sha256sum "$interrupt_project/flake.nix")" ] \
        || fail "upgrade transaction: interrupted apply changed flake.nix"
      [ "$before_lock" = "$(sha256sum "$interrupt_project/flake.lock")" ] \
        || fail "upgrade transaction: interrupted apply changed flake.lock"
      [ "$before_project" = "$(sha256sum "$interrupt_project/nixfied.nix")" ] \
        || fail "upgrade transaction: interrupted apply changed nixfied.nix"
      [ -z "$(find "$interrupt_project" -maxdepth 1 -type f -name '*nixfied-upgrade*' -print -quit)" ] \
        || fail "upgrade transaction: interrupted apply left temporary files"
      rm -rf "$interrupt_project"

      model_project=$(mktemp -d)
      nix run "$checkout#install" -- --root "$model_project" --project-id upgrade-model-failure --name upgrade-model-failure --nixfied-url "$pin" >/dev/null || fail "upgrade transaction: model fixture install failed"
      nix flake lock "$model_project" >/dev/null || fail "upgrade transaction: model fixture lock failed"
      ${pkgs.gnused}/bin/sed -i '/^  nixfied.surface.verbs.smoke = /c\  nixfied.surface.verbs = [ "smoke" ];' "$model_project/nixfied.nix"
      before_flake=$(sha256sum "$model_project/flake.nix")
      before_lock=$(sha256sum "$model_project/flake.lock")
      before_project=$(sha256sum "$model_project/nixfied.nix")
      if nix run "$checkout#upgrade" -- --root "$model_project" --nixfied-url "$pin" >"$model_project/stdout" 2>"$model_project/stderr"; then
        fail "upgrade transaction: incompatible candidate unexpectedly succeeded"
      fi
      grep -Fq "model preflight: failed" "$model_project/stderr" || fail "upgrade transaction: model failure was not reported"
      grep -Fq "upgrade not applied; no project files were changed" "$model_project/stderr" || fail "upgrade transaction: model failure omitted no-mutation status"
      [ "$before_flake" = "$(sha256sum "$model_project/flake.nix")" ] || fail "upgrade transaction: model failure changed flake.nix"
      [ "$before_lock" = "$(sha256sum "$model_project/flake.lock")" ] || fail "upgrade transaction: model failure changed flake.lock"
      [ "$before_project" = "$(sha256sum "$model_project/nixfied.nix")" ] || fail "upgrade transaction: model failure changed nixfied.nix"
      rm -rf "$model_project"

      lock_failure_project=$(mktemp -d)
      nix run "$checkout#install" -- --root "$lock_failure_project" --project-id upgrade-lock-failure --name upgrade-lock-failure --nixfied-url "$pin" >/dev/null || fail "upgrade transaction: lock failure fixture install failed"
      nix flake lock "$lock_failure_project" >/dev/null || fail "upgrade transaction: lock failure fixture lock failed"
      bad_pin="path:$lock_failure_project/missing-nixfied-source"
      before_flake=$(sha256sum "$lock_failure_project/flake.nix")
      before_lock=$(sha256sum "$lock_failure_project/flake.lock")
      before_project=$(sha256sum "$lock_failure_project/nixfied.nix")
      if nix run "$checkout#upgrade" -- --root "$lock_failure_project" --nixfied-url "$bad_pin" >"$lock_failure_project/stdout" 2>"$lock_failure_project/stderr"; then
        fail "upgrade transaction: lock failure unexpectedly succeeded"
      fi
      grep -Fq "candidate lock resolution failed" "$lock_failure_project/stderr" || fail "upgrade transaction: lock failure was not reported"
      [ "$before_flake" = "$(sha256sum "$lock_failure_project/flake.nix")" ] || fail "upgrade transaction: lock failure changed flake.nix"
      [ "$before_lock" = "$(sha256sum "$lock_failure_project/flake.lock")" ] || fail "upgrade transaction: lock failure changed flake.lock"
      [ "$before_project" = "$(sha256sum "$lock_failure_project/nixfied.nix")" ] || fail "upgrade transaction: lock failure changed nixfied.nix"
      rm -rf "$lock_failure_project"

      no_lock_project=$(mktemp -d)
      nix run "$checkout#install" -- --root "$no_lock_project" --project-id upgrade-no-lock --name upgrade-no-lock --nixfied-url "$pin" >/dev/null || fail "upgrade transaction: no-lock fixture install failed"
      nix flake lock "$no_lock_project" >/dev/null || fail "upgrade transaction: no-lock fixture lock failed"
      no_lock_url="path:$checkout"
      before_lock=$(sha256sum "$no_lock_project/flake.lock")
      before_project=$(sha256sum "$no_lock_project/nixfied.nix")
      nix run "$checkout#upgrade" -- --root "$no_lock_project" --nixfied-url "$no_lock_url" --no-lock >"$no_lock_project/stdout" 2>"$no_lock_project/stderr" || fail "upgrade transaction: --no-lock failed"
      grep -Fq "documentation diff: skipped (--no-lock; no candidate lock was produced)" "$no_lock_project/stderr" || fail "upgrade transaction: --no-lock omitted documentation skip"
      grep -Fq "candidate verification: skipped (--no-lock)" "$no_lock_project/stderr" || fail "upgrade transaction: --no-lock omitted verification skip"
      [ "$before_lock" = "$(sha256sum "$no_lock_project/flake.lock")" ] || fail "upgrade transaction: --no-lock changed flake.lock"
      [ "$before_project" = "$(sha256sum "$no_lock_project/nixfied.nix")" ] || fail "upgrade transaction: --no-lock changed nixfied.nix"
      rm -rf "$no_lock_project"
      printf '  upgrade_transaction: ok\n' >&2
    }

    upgrade_transaction_tests

    upgrade_docs_diff_tests() {
      local old_source new_source old_rev new_rev old_git_pin new_git_pin
      local project path_project tar_project unavailable_project
      local old_tar new_tar old_path_pin new_path_pin file_pin
      local before_project

      echo "  upgrade source identity and documentation diff" >&2
      old_source=$(mktemp -d)
      git -C "$checkout" archive HEAD | tar -xf - -C "$old_source" \
        || fail "upgrade docs: old source snapshot failed"
      git -C "$old_source" init -q
      git -C "$old_source" config user.email gate@nixfied
      git -C "$old_source" config user.name "nixfied gate"
      git -C "$old_source" add -A
      git -C "$old_source" commit -q -m old-source

      new_source=$(mktemp -d)
      git -C "$checkout" archive HEAD | tar -xf - -C "$new_source" \
        || fail "upgrade docs: new source snapshot failed"
      printf '\nUpgrade fixture documentation changed here.\n' >>"$new_source/README.md"
      printf '%s\n' '# Upgrade fixture' "" 'This file proves additions stay in the explicit documentation scope.' >"$new_source/docs/upgrade-fixture.md"
      git -C "$new_source" init -q
      git -C "$new_source" config user.email gate@nixfied
      git -C "$new_source" config user.name "nixfied gate"
      git -C "$new_source" add -A
      git -C "$new_source" commit -q -m new-source
      old_rev=$(git -C "$old_source" rev-parse HEAD)
      new_rev=$(git -C "$new_source" rev-parse HEAD)
      old_git_pin="git+file://$old_source?rev=$old_rev&shallow=1"
      new_git_pin="git+file://$new_source?rev=$new_rev&shallow=1"

      project=$(mktemp -d)
      nix run "$checkout#install" -- --root "$project" --project-id upgrade-docs-git --name upgrade-docs-git --nixfied-url "$old_git_pin" >/dev/null \
        || fail "upgrade docs: Git fixture install failed"
      nix flake lock "$project" >/dev/null || fail "upgrade docs: Git fixture lock failed"
      before_project=$(sha256sum "$project/nixfied.nix")
      nix run "$checkout#upgrade" -- --root "$project" --nixfied-url "$new_git_pin" --plan >"$project/plan.stdout" 2>"$project/plan.stderr" \
        || fail "upgrade docs: Git plan failed"
      nix run "$checkout#upgrade" -- --root "$project" --nixfied-url "$new_git_pin" >"$project/apply.stdout" 2>"$project/apply.stderr" \
        || fail "upgrade docs: Git apply failed"
      cmp -s "$project/plan.stdout" "$project/apply.stdout" \
        || fail "upgrade docs: --plan and apply produced different documentation diffs"
      grep -Fq -- '--- old/README.md' "$project/apply.stdout" \
        || fail "upgrade docs: README diff was not emitted"
      grep -Fq -- '--- old/docs/upgrade-fixture.md' "$project/apply.stdout" \
        || fail "upgrade docs: added documentation file was not emitted"
      grep -Fq -- '+++ new/docs/upgrade-fixture.md' "$project/apply.stdout" \
        || fail "upgrade docs: added documentation label was not stable"
      [ "$(grep '^--- old/' "$project/apply.stdout" | head -n 1)" = '--- old/README.md' ] \
        || fail "upgrade docs: scoped files were not in canonical order"
      ! grep -Fq -- 'nix/compiler/' "$project/apply.stdout" \
        || fail "upgrade docs: source code entered the diff"
      ! grep -Fq -- 'views/docs.md' "$project/apply.stdout" \
        || fail "upgrade docs: generated model view entered the diff"
      ! grep -Fq -- '/nix/store/' "$project/apply.stdout" \
        || fail "upgrade docs: store paths entered the diff"
      ! grep -Fq -- 'model preflight' "$project/apply.stdout" \
        || fail "upgrade docs: status leaked onto stdout"
      grep -Fq -- "\"rev\":\"$old_rev\"" "$project/apply.stderr" \
        || fail "upgrade docs: old locked revision was not reported"
      grep -Fq -- "\"rev\":\"$new_rev\"" "$project/apply.stderr" \
        || fail "upgrade docs: candidate locked revision was not reported"
      grep -Fq -- 'model preflight: passed' "$project/apply.stderr" \
        || fail "upgrade docs: candidate preflight status was not reported"
      grep -Fq -- 'preserved (project-owned): nixfied.nix' "$project/apply.stderr" \
        || fail "upgrade docs: ownership status was not reported"
      [ "$before_project" = "$(sha256sum "$project/nixfied.nix")" ] \
        || fail "upgrade docs: Git upgrade changed nixfied.nix"
      jq -e --arg rev "$new_rev" '.nodes[.nodes[.root].inputs.nixfied].locked.rev == $rev' "$project/flake.lock" >/dev/null \
        || fail "upgrade docs: candidate Git revision was not applied"
      rm -rf "$project"

      old_path_pin="path:$old_source"
      new_path_pin="path:$new_source"
      path_project=$(mktemp -d)
      nix run "$checkout#install" -- --root "$path_project" --project-id upgrade-docs-path --name upgrade-docs-path --nixfied-url "$old_path_pin" >/dev/null \
        || fail "upgrade docs: path fixture install failed"
      nix flake lock "$path_project" >/dev/null || fail "upgrade docs: path fixture lock failed"
      nix run "$checkout#upgrade" -- --root "$path_project" --nixfied-url "$new_path_pin" >"$path_project/stdout" 2>"$path_project/stderr" \
        || fail "upgrade docs: path upgrade failed"
      grep -Fq -- '"type":"path"' "$path_project/stderr" \
        || fail "upgrade docs: path locked identity was not reported"
      ! grep -Fq -- '"rev"' "$path_project/stderr" \
        || fail "upgrade docs: path identity fabricated a revision"
      grep -Fq -- 'Upgrade fixture documentation changed here.' "$path_project/stdout" \
        || fail "upgrade docs: path source diff was not emitted"
      rm -rf "$path_project"

      old_tar="$new_source/../nixfied-old.tar.gz"
      new_tar="$new_source/../nixfied-new.tar.gz"
      tar --exclude=.git -C "$old_source" -czf "$old_tar" . || fail "upgrade docs: old tarball creation failed"
      tar --exclude=.git -C "$new_source" -czf "$new_tar" . || fail "upgrade docs: new tarball creation failed"
      file_pin="file://$old_tar"
      tar_project=$(mktemp -d)
      nix run "$checkout#install" -- --root "$tar_project" --project-id upgrade-docs-tar --name upgrade-docs-tar --nixfied-url "$file_pin" >/dev/null \
        || fail "upgrade docs: tarball fixture install failed"
      nix flake lock "$tar_project" >/dev/null || fail "upgrade docs: tarball fixture lock failed"
      nix run "$checkout#upgrade" -- --root "$tar_project" --nixfied-url "file://$new_tar" >"$tar_project/stdout" 2>"$tar_project/stderr" \
        || fail "upgrade docs: tarball upgrade failed"
      grep -Fq -- '"type":"tarball"' "$tar_project/stderr" \
        || fail "upgrade docs: tarball locked identity was not reported"
      grep -Fq -- 'narHash' "$tar_project/stderr" \
        || fail "upgrade docs: tarball content hash was not reported"
      grep -Fq -- '--- old/README.md' "$tar_project/stdout" \
        || fail "upgrade docs: tarball source diff was not emitted"
      rm -rf "$tar_project"

      unavailable_project=$(mktemp -d)
      unavailable_source=$(mktemp -d)
      git -C "$checkout" archive HEAD | tar -xf - -C "$unavailable_source" \
        || fail "upgrade docs: unavailable source snapshot failed"
      nix run "$checkout#install" -- --root "$unavailable_project" --project-id upgrade-docs-unavailable --name upgrade-docs-unavailable --nixfied-url "path:$unavailable_source" >/dev/null \
        || fail "upgrade docs: unavailable fixture install failed"
      nix flake lock "$unavailable_project" >/dev/null || fail "upgrade docs: unavailable fixture lock failed"
      jq '.nodes[.nodes[.root].inputs.nixfied].locked.narHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="' "$unavailable_project/flake.lock" >"$unavailable_project/flake.lock.invalid" \
        || fail "upgrade docs: unavailable lock fixture could not be prepared"
      mv "$unavailable_project/flake.lock.invalid" "$unavailable_project/flake.lock"
      rm -rf "$unavailable_source"
      nix run "$checkout#upgrade" -- --root "$unavailable_project" --nixfied-url "$new_path_pin" >"$unavailable_project/stdout" 2>"$unavailable_project/stderr" \
        || fail "upgrade docs: unavailable source prevented a valid upgrade"
      grep -Fq -- 'documentation diff unavailable: old source materialization failed' "$unavailable_project/stderr" \
        || fail "upgrade docs: unavailable source warning was missing"
      grep -Fq -- '--- DOCUMENTATION DIFF UNAVAILABLE ---' "$unavailable_project/stdout" \
        || fail "upgrade docs: unavailable source was reported as a diff"
      ! grep -Fq -- 'NO CHECKED-IN DOCUMENTATION CHANGED' "$unavailable_project/stdout" \
        || fail "upgrade docs: unavailable source was reported as empty"
      grep -Fq -- 'model preflight: passed' "$unavailable_project/stderr" \
        || fail "upgrade docs: unavailable source incorrectly failed candidate preflight"
      rm -rf "$unavailable_project" "$old_source" "$new_source" "$old_tar" "$new_tar"
      printf '  upgrade_docs_diff: ok\n' >&2
    }

    upgrade_docs_diff_tests

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
