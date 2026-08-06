# Nix-layer tests: checks that exercise the Nix compiler and install tooling.
# These stay in bash because they perform open-ended Nix builds and external
# fetches; expressing them as runtime tasks would exceed the bounded role of
# those framework tasks and blur which layer is under test.
{ pkgs, debugRuntime }:
pkgs.writeShellApplication {
  name = "nixfied-gate-nix";
  runtimeInputs = [
    pkgs.nix
    pkgs.git
    pkgs.jq
    pkgs.coreutils
    pkgs.diffutils
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

    rt="${debugRuntime}/bin/nixfied-runtime"

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

    echo "  adoption (#install and runtime controls against a throwaway repo)" >&2
    upgrade_fixture="$checkout/nix/fixtures/upgrade-golden"
    upgrade_manifest="$upgrade_fixture/manifest.json"
    upgrade_expected="$upgrade_fixture/expected.diff"
    upgrade_old_archive="$upgrade_fixture/nixfied-old.tar.gz"
    upgrade_new_archive="$upgrade_fixture/nixfied-new.tar.gz"
    upgrade_old_url="file://$upgrade_old_archive"
    upgrade_new_url="file://$upgrade_new_archive"
    upgrade_transaction_tests() {
      local project model_project lock_failure_project no_lock_project
      local before_flake before_lock before_project bad_pin no_lock_url

      echo "  upgrade transaction and mode semantics" >&2

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
      NIXFIED_UPGRADE_TEST_PAUSE_BEFORE_APPLY=10 nix run "$checkout#upgrade" -- \
        --root "$project" --nixfied-url "$race_candidate_pin" \
        >"$project/race-one.stdout" 2>"$project/race-one.stderr" &
      race_one=$!
      race_paused=0
      for attempt in $(seq 1 120); do
        : "$attempt"
        if grep -Fq "test pause before apply" "$project/race-one.stderr"; then
          race_paused=1
          break
        fi
        kill -0 "$race_one" 2>/dev/null || break
        sleep 0.25
      done
      if [ "$race_paused" -ne 1 ]; then
        kill "$race_one" 2>/dev/null || true
        fail "upgrade transaction: concurrent fixture never reached the guarded pre-apply window"
      fi
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
      grep -hEq "changed concurrently|concurrently|changed while the candidate was being prepared" "$project/race-one.stderr" "$project/race-two.stderr" \
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
        grep -Fq '  type: git' "$project/race-one.stderr" "$project/race-two.stderr" \
          || fail "upgrade transaction: concurrent Git candidate identity was not reported"
      else
        grep -Fq '  type: path' "$project/race-one.stderr" "$project/race-two.stderr" \
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
      grep -Fq "candidate verification: failed (model preflight)" "$model_project/stderr" || fail "upgrade transaction: model failure was not reported"
      grep -Fq "upgrade applied: no" "$model_project/stderr" || fail "upgrade transaction: model failure omitted apply status"
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
      no_lock_url="$pin"
      before_lock=$(sha256sum "$no_lock_project/flake.lock")
      before_project=$(sha256sum "$no_lock_project/nixfied.nix")
      nix run "$checkout#upgrade" -- --root "$no_lock_project" --nixfied-url "$no_lock_url" --no-lock >"$no_lock_project/stdout" 2>"$no_lock_project/stderr" || fail "upgrade transaction: --no-lock failed"
      grep -Fq "documentation diff: skipped (--no-lock; no candidate lock was produced)" "$no_lock_project/stderr" || fail "upgrade transaction: --no-lock omitted documentation skip"
      grep -Fq "candidate verification: skipped (--no-lock)" "$no_lock_project/stderr" || fail "upgrade transaction: --no-lock omitted verification skip"
      grep -Fq "upgrade applied: no (flake.nix already matched)" "$no_lock_project/stderr" || fail "upgrade transaction: --no-lock omitted already-matched status"
      grep -Fq "unchanged: flake.nix" "$no_lock_project/stderr" || fail "upgrade transaction: --no-lock omitted unchanged flake status"
      grep -Fq "preserved: nixfied.nix (project-owned)" "$no_lock_project/stderr" || fail "upgrade transaction: --no-lock omitted project ownership"
      [ "$before_lock" = "$(sha256sum "$no_lock_project/flake.lock")" ] || fail "upgrade transaction: --no-lock changed flake.lock"
      [ "$before_project" = "$(sha256sum "$no_lock_project/nixfied.nix")" ] || fail "upgrade transaction: --no-lock changed nixfied.nix"
      rm -rf "$no_lock_project"
      printf '  upgrade_transaction: ok\n' >&2
    }

    upgrade_transaction_tests

    upgrade_versioned_tests() {
      local source_root old_source new_source git_root old_git_source new_git_source
      local unavailable_source old_path_pin new_path_pin old_git_pin new_git_pin
      local old_git_rev new_git_rev
      local failure_project unsupported_project success_project path_project git_project unavailable_project
      local scope_root scope_old_source scope_new_source scope_project
      local scope_old_patch scope_new_patch scope_expected scope_expected_hash
      local before_flake before_lock before_project state run_status expected_empty
      local old_commit new_commit old_archive_hash new_archive_hash expected_diff_hash

      sha256_file() {
        sha256sum "$1" | cut -d ' ' -f1
      }

      assert_upgrade_golden() {
        local output="$1" label="$2"
        if ! cmp -s "$upgrade_expected" "$output"; then
          echo "  golden mismatch: $label" >&2
          diff -u "$upgrade_expected" "$output" >&2 || true
          fail "upgrade golden: $label differed from expected.diff"
        fi
      }

      assert_upgrade_unchanged() {
        local root="$1" expected_flake="$2" expected_lock="$3" expected_project="$4" label="$5"
        [ "$expected_flake" = "$(sha256sum "$root/flake.nix")" ] \
          || fail "upgrade golden: $label changed flake.nix"
        [ "$expected_lock" = "$(sha256sum "$root/flake.lock")" ] \
          || fail "upgrade golden: $label changed flake.lock"
        [ "$expected_project" = "$(sha256sum "$root/nixfied.nix")" ] \
          || fail "upgrade golden: $label changed nixfied.nix"
      }

      assert_blank_after() {
        local path="$1" marker="$2" label="$3" line found=0
        while IFS= read -r line || [[ -n "$line" ]]; do
          if [[ "$found" -eq 1 ]]; then
            [[ -z "$line" ]] \
              || fail "upgrade golden: $label did not separate status sections"
            return 0
          fi
          if [[ "$line" == "$marker" ]]; then
            found=1
          fi
        done <"$path"
        fail "upgrade golden: $label status marker was missing"
      }

      make_git_fixture() {
        local archive="$1" destination="$2"
        mkdir "$destination"
        tar -xzf "$archive" -C "$destination" \
          || fail "upgrade golden: Git fixture extraction failed"
        git -C "$destination" init -q
        git -C "$destination" config user.email gate@nixfied
        git -C "$destination" config user.name "nixfied gate"
        git -C "$destination" add -A
        GIT_AUTHOR_DATE=2000-01-01T00:00:00Z GIT_COMMITTER_DATE=2000-01-01T00:00:00Z \
          git -C "$destination" commit -q -m fixture
      }

      echo "  upgrade pinned versions and documentation golden" >&2
      [ -f "$upgrade_manifest" ] || fail "upgrade golden: manifest is missing"
      [ -f "$upgrade_expected" ] || fail "upgrade golden: expected diff is missing"
      [ -f "$upgrade_old_archive" ] || fail "upgrade golden: old archive is missing"
      [ -f "$upgrade_new_archive" ] || fail "upgrade golden: new archive is missing"
      scope_old_patch="$upgrade_fixture/scope-old.patch"
      scope_new_patch="$upgrade_fixture/scope-new.patch"
      scope_expected="$upgrade_fixture/scope.expected.diff"
      [ -f "$scope_old_patch" ] || fail "upgrade golden: scope old patch is missing"
      [ -f "$scope_new_patch" ] || fail "upgrade golden: scope new patch is missing"
      [ -f "$scope_expected" ] || fail "upgrade golden: scope expected diff is missing"
      jq -e '
        .archiveFormat == "tar.gz"
        and .normalization.sort == "name"
        and .normalization.mtime == "1970-01-01T00:00:00Z"
        and .normalization.owner == 0
        and .normalization.group == 0
        and .normalization.numericOwner == true
        and .normalization.gzip == "-n"
      ' "$upgrade_manifest" >/dev/null \
        || fail "upgrade golden: manifest normalization is not deterministic"
      old_commit=$(jq -er '.old.commit' "$upgrade_manifest") \
        || fail "upgrade golden: old commit is missing"
      new_commit=$(jq -er '.new.commit' "$upgrade_manifest") \
        || fail "upgrade golden: new commit is missing"
      old_archive_hash=$(jq -er '.old.archiveSha256' "$upgrade_manifest") \
        || fail "upgrade golden: old archive hash is missing"
      new_archive_hash=$(jq -er '.new.archiveSha256' "$upgrade_manifest") \
        || fail "upgrade golden: new archive hash is missing"
      upgrade_old_nar_hash=$(jq -er '.old.narHash' "$upgrade_manifest") \
        || fail "upgrade golden: old NAR hash is missing"
      upgrade_new_nar_hash=$(jq -er '.new.narHash' "$upgrade_manifest") \
        || fail "upgrade golden: new NAR hash is missing"
      expected_diff_hash=$(jq -er '.expectedDiffSha256' "$upgrade_manifest") \
        || fail "upgrade golden: expected diff hash is missing"
      scope_expected_hash=$(jq -er '.scopeExpectedDiffSha256' "$upgrade_manifest") \
        || fail "upgrade golden: scope expected diff hash is missing"
      [ "$old_commit" = "8e1fa56d9423b93b89f9aabf4be86c2081da0362" ] \
        || fail "upgrade golden: old source commit drifted"
      [ "$new_commit" = "0aaf8740e7ef8aa5ee487085bac7be0dd5998007" ] \
        || fail "upgrade golden: new source commit drifted"
      [ "$old_archive_hash" = "013005b8e74778217891ae7f5199191eafef1edd7ec4ea2430389bf262349c0c" ] \
        || fail "upgrade golden: old archive provenance drifted"
      [ "$new_archive_hash" = "b63ce314aef0f41b7394ff7388e1ba32b46f6db0c8ecfc105c393a9705de183d" ] \
        || fail "upgrade golden: new archive provenance drifted"
      [ "$(sha256_file "$upgrade_old_archive")" = "$old_archive_hash" ] \
        || fail "upgrade golden: old archive checksum mismatch"
      [ "$(sha256_file "$upgrade_new_archive")" = "$new_archive_hash" ] \
        || fail "upgrade golden: new archive checksum mismatch"
      [ "$(sha256_file "$upgrade_expected")" = "$expected_diff_hash" ] \
        || fail "upgrade golden: expected diff checksum mismatch"
      [ "$scope_expected_hash" = "7d8ab0971df7d8f494aeff47800b3dece6e20638e97f44397935ec136db31589" ] \
        || fail "upgrade golden: scope expected diff provenance drifted"
      [ "$(sha256_file "$scope_expected")" = "$scope_expected_hash" ] \
        || fail "upgrade golden: scope expected diff checksum mismatch"

      failure_project=$(mktemp -d)
      nix run "$upgrade_old_url#install" -- \
        --root "$failure_project" --project-id upgrade-golden-failure --name upgrade-golden-failure \
        --nixfied-url "$upgrade_old_url" >/dev/null \
        || fail "upgrade golden: historical install failed"
      grep -Fq 'nixfied.surface.verbs = [ "smoke" ];' "$failure_project/nixfied.nix" \
        || fail "upgrade golden: historical installer did not produce the list-form declaration"
      nix flake lock "$failure_project" >/dev/null \
        || fail "upgrade golden: historical failure fixture lock failed"
      jq -e --arg expected "$upgrade_old_nar_hash" \
        '.nodes[.nodes[.root].inputs.nixfied].locked.narHash == $expected' \
        "$failure_project/flake.lock" >/dev/null \
        || fail "upgrade golden: historical failure fixture did not lock the pinned old source"
      before_flake=$(sha256sum "$failure_project/flake.nix")
      before_lock=$(sha256sum "$failure_project/flake.lock")
      before_project=$(sha256sum "$failure_project/nixfied.nix")
      state="$failure_project/runtime-state"
      run_status=0
      ( NIXFIED_STATE_DIR="$state" nix run "$checkout#upgrade" -- \
          --root "$failure_project" --nixfied-url "$upgrade_new_url" --plan \
          >"$failure_project/plan.stdout" 2>"$failure_project/plan.stderr" ) \
        || run_status=$?
      [ "$run_status" -eq 5 ] || fail "upgrade golden: incompatible plan returned $run_status instead of 5"
      assert_upgrade_golden "$failure_project/plan.stdout" "incompatible plan"
      grep -Fq '  type: tarball' "$failure_project/plan.stderr" \
        || fail "upgrade golden: tarball identity was not reported for the incompatible plan"
      grep -Fq "  narHash: $upgrade_old_nar_hash" "$failure_project/plan.stderr" \
        || fail "upgrade golden: old tarball NAR hash was not reported"
      grep -Fq "  narHash: $upgrade_new_nar_hash" "$failure_project/plan.stderr" \
        || fail "upgrade golden: candidate tarball NAR hash was not reported"
      grep -Fq 'candidate verification: failed (model preflight)' "$failure_project/plan.stderr" \
        || fail "upgrade golden: incompatible plan omitted model preflight failure"
      grep -Fq 'upgrade applied: no' "$failure_project/plan.stderr" \
        || fail "upgrade golden: incompatible plan omitted apply status"
      grep -Fq 'upgrade not applied; no project files were changed' "$failure_project/plan.stderr" \
        || fail "upgrade golden: incompatible plan omitted no-mutation status"
      assert_upgrade_unchanged "$failure_project" "$before_flake" "$before_lock" "$before_project" "incompatible plan"
      [ ! -e "$state" ] || fail "upgrade golden: incompatible plan materialized runtime state"

      run_status=0
      ( NIXFIED_STATE_DIR="$state" nix run "$checkout#upgrade" -- \
          --root "$failure_project" --nixfied-url "$upgrade_new_url" \
          >"$failure_project/apply.stdout" 2>"$failure_project/apply.stderr" ) \
        || run_status=$?
      [ "$run_status" -eq 5 ] || fail "upgrade golden: incompatible apply returned $run_status instead of 5"
      assert_upgrade_golden "$failure_project/apply.stdout" "incompatible apply"
      grep -Fq 'candidate verification: failed (model preflight)' "$failure_project/apply.stderr" \
        || fail "upgrade golden: incompatible apply omitted model preflight failure"
      grep -Fq 'upgrade applied: no' "$failure_project/apply.stderr" \
        || fail "upgrade golden: incompatible apply omitted apply status"
      grep -Fq 'upgrade not applied; no project files were changed' "$failure_project/apply.stderr" \
        || fail "upgrade golden: incompatible apply omitted no-mutation status"
      assert_upgrade_unchanged "$failure_project" "$before_flake" "$before_lock" "$before_project" "incompatible apply"
      [ ! -e "$state" ] || fail "upgrade golden: incompatible apply materialized runtime state"
      rm -rf "$failure_project"

      unsupported_project=$(mktemp -d)
      nix run "$upgrade_old_url#install" -- \
        --root "$unsupported_project" --project-id upgrade-golden-unsupported --name upgrade-golden-unsupported \
        --nixfied-url "$upgrade_old_url" >/dev/null \
        || fail "upgrade golden: unsupported identity install failed"
      nix flake lock "$unsupported_project" >/dev/null \
        || fail "upgrade golden: unsupported identity fixture lock failed"
      jq '.nodes[.nodes[.root].inputs.nixfied].locked.type = "sourcehut"' \
        "$unsupported_project/flake.lock" >"$unsupported_project/flake.lock.invalid" \
        || fail "upgrade golden: unsupported identity lock fixture could not be prepared"
      mv "$unsupported_project/flake.lock.invalid" "$unsupported_project/flake.lock"
      before_flake=$(sha256sum "$unsupported_project/flake.nix")
      before_lock=$(sha256sum "$unsupported_project/flake.lock")
      before_project=$(sha256sum "$unsupported_project/nixfied.nix")
      state="$unsupported_project/runtime-state"
      run_status=0
      ( NIXFIED_STATE_DIR="$state" nix run "$checkout#upgrade" -- \
          --root "$unsupported_project" --nixfied-url "$upgrade_new_url" --plan \
          >"$unsupported_project/stdout" 2>"$unsupported_project/stderr" ) \
        || run_status=$?
      [ "$run_status" -eq 4 ] \
        || fail "upgrade golden: unsupported identity returned $run_status instead of 4"
      grep -Fq 'sourcehut' "$unsupported_project/stderr" \
        || fail "upgrade golden: unsupported identity was not named"
      grep -Fq 'candidate lock resolution failed' "$unsupported_project/stderr" \
        || fail "upgrade golden: unsupported identity omitted lock failure"
      grep -Fq 'upgrade not applied; no project files were changed' "$unsupported_project/stderr" \
        || fail "upgrade golden: unsupported identity omitted no-mutation status"
      ! grep -Fq '"rev"' "$unsupported_project/stderr" \
        || fail "upgrade golden: unsupported identity fabricated a revision"
      [ ! -s "$unsupported_project/stdout" ] \
        || fail "upgrade golden: unsupported identity emitted a partial documentation report"
      assert_upgrade_unchanged "$unsupported_project" "$before_flake" "$before_lock" "$before_project" "unsupported identity"
      [ ! -e "$state" ] || fail "upgrade golden: unsupported identity materialized runtime state"
      rm -rf "$unsupported_project"

      success_project=$(mktemp -d)
      nix run "$upgrade_old_url#install" -- \
        --root "$success_project" --project-id upgrade-golden-success --name upgrade-golden-success \
        --nixfied-url "$upgrade_old_url" >/dev/null \
        || fail "upgrade golden: historical compatible install failed"
      grep -Fq 'nixfied.surface.verbs = [ "smoke" ];' "$success_project/nixfied.nix" \
        || fail "upgrade golden: compatible fixture did not start from the list-form declaration"
      ${pkgs.gnused}/bin/sed -i \
        '/^  nixfied.surface.verbs = \[ "smoke" \];$/d' \
        "$success_project/nixfied.nix"
      ! grep -Fq 'nixfied.surface.verbs = [ "smoke" ];' "$success_project/nixfied.nix" \
        || fail "upgrade golden: compatible fixture retained the incompatible list declaration"
      nix flake lock "$success_project" >/dev/null \
        || fail "upgrade golden: historical compatible fixture lock failed"
      before_flake=$(sha256sum "$success_project/flake.nix")
      before_lock=$(sha256sum "$success_project/flake.lock")
      before_project=$(sha256sum "$success_project/nixfied.nix")
      state="$success_project/runtime-state"
      nix run "$checkout#upgrade" -- \
        --root "$success_project" --nixfied-url "$upgrade_new_url" --plan \
        >"$success_project/plan.stdout" 2>"$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan failed"
      assert_upgrade_golden "$success_project/plan.stdout" "compatible plan"
      grep -Fq 'candidate verification: passed (model preflight)' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted model preflight success"
      assert_blank_after "$success_project/plan.stderr" \
        'candidate verification: passed (model preflight)' 'compatible plan'
      grep -Fq 'upgrade applied: no (--plan)' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted plan apply status"
      grep -Fq 'plan: no project files changed' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted no-mutation status"
      grep -Fq 'would change: flake.nix' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted flake.nix plan status"
      grep -Fq 'would change: flake.lock (nixfied input)' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted flake.lock plan status"
      grep -Fq 'preserved: nixfied.nix (project-owned)' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted project ownership"
      grep -Fq 'post-upgrade validation: not run (--plan)' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted post-upgrade validation status"
      grep -Fxq 'next:' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted next-step header"
      grep -Fq 'rerun upgrade without --plan' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted apply next step"
      grep -Fq '  nix build ' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted model build next step"
      grep -Fq '  nix run ' "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted model-check next step"
      grep -Fq "  narHash: $upgrade_old_nar_hash" "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted old NAR identity"
      grep -Fq "  narHash: $upgrade_new_nar_hash" "$success_project/plan.stderr" \
        || fail "upgrade golden: compatible plan omitted candidate NAR identity"
      assert_upgrade_unchanged "$success_project" "$before_flake" "$before_lock" "$before_project" "compatible plan"
      [ ! -e "$state" ] || fail "upgrade golden: compatible plan materialized runtime state"

      NIXFIED_STATE_DIR="$state" nix run "$checkout#upgrade" -- \
        --root "$success_project" --nixfied-url "$upgrade_new_url" \
        >"$success_project/apply.stdout" 2>"$success_project/apply.stderr" \
        || fail "upgrade golden: compatible apply failed"
      assert_upgrade_golden "$success_project/apply.stdout" "compatible apply"
      grep -Fq 'candidate verification: passed (model preflight)' "$success_project/apply.stderr" \
        || fail "upgrade golden: compatible apply omitted model preflight success"
      assert_blank_after "$success_project/apply.stderr" \
        'candidate verification: passed (model preflight)' 'compatible apply'
      grep -Fq 'upgrade applied: yes' "$success_project/apply.stderr" \
        || fail "upgrade golden: compatible apply omitted apply status"
      grep -Fq 'changed: flake.nix' "$success_project/apply.stderr" \
        || fail "upgrade golden: compatible apply did not report flake.nix"
      grep -Fq 'flake.lock (nixfied input)' "$success_project/apply.stderr" \
        || fail "upgrade golden: compatible apply did not report flake.lock"
      grep -Fq 'preserved: nixfied.nix (project-owned)' "$success_project/apply.stderr" \
        || fail "upgrade golden: compatible apply did not report project ownership"
      grep -Fq 'post-upgrade validation: not run' "$success_project/apply.stderr" \
        || fail "upgrade golden: compatible apply omitted post-upgrade validation status"
      grep -Fxq 'next:' "$success_project/apply.stderr" \
        || fail "upgrade golden: compatible apply omitted next-step header"
      grep -Fq '  nix build ' "$success_project/apply.stderr" \
        || fail "upgrade golden: compatible apply omitted model build next step"
      grep -Fq '  nix run ' "$success_project/apply.stderr" \
        || fail "upgrade golden: compatible apply omitted model-check next step"
      grep -Fq "$upgrade_new_url" "$success_project/flake.nix" \
        || fail "upgrade golden: compatible apply did not apply the pinned new URL"
      jq -e --arg expected "$upgrade_new_nar_hash" \
        '.nodes[.nodes[.root].inputs.nixfied].locked.narHash == $expected' \
        "$success_project/flake.lock" >/dev/null \
        || fail "upgrade golden: compatible apply did not apply the pinned new lock"
      [ "$before_project" = "$(sha256sum "$success_project/nixfied.nix")" ] \
        || fail "upgrade golden: compatible apply changed nixfied.nix"
      [ ! -e "$state" ] \
        || fail "upgrade golden: compatible apply materialized runtime state"
      nix build --no-link "$success_project#model" >/dev/null \
        || fail "upgrade golden: compatible applied model did not build"
      nix run --no-write-lock-file "$success_project#model-check" >/dev/null \
        || fail "upgrade golden: compatible applied model-check failed"

      expected_empty="$success_project/empty.expected"
      printf '%s\n' \
        '--- BEGIN NIXFIED DOCUMENTATION DIFF ---' \
        '--- NO CHECKED-IN DOCUMENTATION CHANGED ---' \
        '--- END NIXFIED DOCUMENTATION DIFF ---' \
        >"$expected_empty"
      before_flake=$(sha256sum "$success_project/flake.nix")
      before_lock=$(sha256sum "$success_project/flake.lock")
      before_project=$(sha256sum "$success_project/nixfied.nix")
      nix run "$checkout#upgrade" -- \
        --root "$success_project" --nixfied-url "$upgrade_new_url" --plan \
        >"$success_project/empty.stdout" 2>"$success_project/empty.stderr" \
        || fail "upgrade golden: unchanged source plan failed"
      cmp -s "$expected_empty" "$success_project/empty.stdout" \
        || fail "upgrade golden: unchanged source did not emit the exact empty marker"
      grep -Fq 'plan: no project files changed' "$success_project/empty.stderr" \
        || fail "upgrade golden: unchanged source omitted no-mutation status"
      assert_upgrade_unchanged "$success_project" "$before_flake" "$before_lock" "$before_project" "unchanged source plan"
      [ ! -e "$state" ] || fail "upgrade golden: unchanged source plan materialized runtime state"
      rm -rf "$success_project"

      scope_root=$(mktemp -d)
      scope_old_source="$scope_root/old"
      scope_new_source="$scope_root/new"
      mkdir "$scope_old_source" "$scope_new_source"
      tar -xzf "$upgrade_new_archive" -C "$scope_old_source" \
        || fail "upgrade golden: scope old extraction failed"
      tar -xzf "$upgrade_new_archive" -C "$scope_new_source" \
        || fail "upgrade golden: scope new extraction failed"
      git -C "$scope_old_source" apply --unidiff-zero --unsafe-paths "$scope_old_patch" \
        || fail "upgrade golden: scope old patch failed"
      git -C "$scope_new_source" apply --unidiff-zero --unsafe-paths "$scope_new_patch" \
        || fail "upgrade golden: scope new patch failed"
      scope_project=$(mktemp -d)
      nix run "$checkout#install" -- \
        --root "$scope_project" --project-id upgrade-golden-scope --name upgrade-golden-scope \
        --nixfied-url "path:$scope_old_source" >/dev/null \
        || fail "upgrade golden: scope fixture install failed"
      nix flake lock "$scope_project" >/dev/null \
        || fail "upgrade golden: scope fixture lock failed"
      before_flake=$(sha256sum "$scope_project/flake.nix")
      before_lock=$(sha256sum "$scope_project/flake.lock")
      before_project=$(sha256sum "$scope_project/nixfied.nix")
      state="$scope_project/runtime-state"
      nix run "$checkout#upgrade" -- \
        --root "$scope_project" --nixfied-url "path:$scope_new_source" --plan \
        >"$scope_project/stdout" 2>"$scope_project/stderr" \
        || fail "upgrade golden: documentation scope plan failed"
      cmp -s "$scope_expected" "$scope_project/stdout" \
        || fail "upgrade golden: README/addition/deletion scope diff drifted"
      grep -Fq '  type: path' "$scope_project/stderr" \
        || fail "upgrade golden: scope fixture path identity was not reported"
      ! grep -Fq '  rev:' "$scope_project/stderr" \
        || fail "upgrade golden: scope fixture path identity fabricated a revision"
      grep -Fq 'candidate verification: passed (model preflight)' "$scope_project/stderr" \
        || fail "upgrade golden: documentation scope plan omitted model preflight success"
      assert_upgrade_unchanged "$scope_project" "$before_flake" "$before_lock" "$before_project" "documentation scope plan"
      [ ! -e "$state" ] || fail "upgrade golden: documentation scope plan materialized runtime state"
      rm -rf "$scope_project" "$scope_root"

      source_root=$(mktemp -d)
      old_source="$source_root/old"
      new_source="$source_root/new"
      mkdir "$old_source" "$new_source"
      tar -xzf "$upgrade_old_archive" -C "$old_source" \
        || fail "upgrade golden: path fixture old extraction failed"
      tar -xzf "$upgrade_new_archive" -C "$new_source" \
        || fail "upgrade golden: path fixture new extraction failed"
      old_path_pin="path:$old_source"
      new_path_pin="path:$new_source"

      path_project=$(mktemp -d)
      nix run "$checkout#install" -- \
        --root "$path_project" --project-id upgrade-golden-path --name upgrade-golden-path \
        --nixfied-url "$old_path_pin" >/dev/null \
        || fail "upgrade golden: path fixture install failed"
      nix flake lock "$path_project" >/dev/null \
        || fail "upgrade golden: path fixture lock failed"
      nix run "$checkout#upgrade" -- \
        --root "$path_project" --nixfied-url "$new_path_pin" \
        >"$path_project/stdout" 2>"$path_project/stderr" \
        || fail "upgrade golden: path fixture upgrade failed"
      assert_upgrade_golden "$path_project/stdout" "path upgrade"
      grep -Fq '  type: path' "$path_project/stderr" \
        || fail "upgrade golden: path identity was not reported"
      ! grep -Fq '  rev:' "$path_project/stderr" \
        || fail "upgrade golden: path identity fabricated a revision"
      jq -e '
        .nodes[.nodes[.root].inputs.nixfied].locked as $locked
        | $locked.type == "path" and ($locked | has("rev") | not)
      ' "$path_project/flake.lock" >/dev/null \
        || fail "upgrade golden: path lock identity was not preserved"
      rm -rf "$path_project"

      git_root=$(mktemp -d)
      old_git_source="$git_root/old"
      new_git_source="$git_root/new"
      make_git_fixture "$upgrade_old_archive" "$old_git_source"
      make_git_fixture "$upgrade_new_archive" "$new_git_source"
      old_git_rev=$(git -C "$old_git_source" rev-parse HEAD)
      new_git_rev=$(git -C "$new_git_source" rev-parse HEAD)
      old_git_pin="git+file://$old_git_source?rev=$old_git_rev&shallow=1"
      new_git_pin="git+file://$new_git_source?rev=$new_git_rev&shallow=1"

      git_project=$(mktemp -d)
      nix run "$checkout#install" -- \
        --root "$git_project" --project-id upgrade-golden-git --name upgrade-golden-git \
        --nixfied-url "$old_git_pin" >/dev/null \
        || fail "upgrade golden: Git fixture install failed"
      nix flake lock "$git_project" >/dev/null \
        || fail "upgrade golden: Git fixture lock failed"
      before_flake=$(sha256sum "$git_project/flake.nix")
      before_lock=$(sha256sum "$git_project/flake.lock")
      before_project=$(sha256sum "$git_project/nixfied.nix")
      nix run "$checkout#upgrade" -- \
        --root "$git_project" --nixfied-url "$new_git_pin" --plan \
        >"$git_project/plan.stdout" 2>"$git_project/plan.stderr" \
        || fail "upgrade golden: Git plan failed"
      assert_upgrade_golden "$git_project/plan.stdout" "Git plan"
      grep -Fq '  type: git' "$git_project/plan.stderr" \
        || fail "upgrade golden: Git identity was not reported"
      grep -Fq "  rev: $old_git_rev" "$git_project/plan.stderr" \
        || fail "upgrade golden: old Git revision was not reported"
      grep -Fq "  rev: $new_git_rev" "$git_project/plan.stderr" \
        || fail "upgrade golden: candidate Git revision was not reported"
      assert_upgrade_unchanged "$git_project" "$before_flake" "$before_lock" "$before_project" "Git plan"
      nix run "$checkout#upgrade" -- \
        --root "$git_project" --nixfied-url "$new_git_pin" \
        >"$git_project/apply.stdout" 2>"$git_project/apply.stderr" \
        || fail "upgrade golden: Git apply failed"
      assert_upgrade_golden "$git_project/apply.stdout" "Git apply"
      grep -Fq "  rev: $old_git_rev" "$git_project/apply.stderr" \
        || fail "upgrade golden: Git apply omitted old revision"
      grep -Fq "  rev: $new_git_rev" "$git_project/apply.stderr" \
        || fail "upgrade golden: Git apply omitted candidate revision"
      jq -e --arg expected "$new_git_rev" \
        '.nodes[.nodes[.root].inputs.nixfied].locked.rev == $expected' \
        "$git_project/flake.lock" >/dev/null \
        || fail "upgrade golden: Git apply did not apply the candidate revision"
      [ "$before_project" = "$(sha256sum "$git_project/nixfied.nix")" ] \
        || fail "upgrade golden: Git apply changed nixfied.nix"
      rm -rf "$git_project"

      unavailable_project=$(mktemp -d)
      unavailable_source="$source_root/unavailable-old"
      mkdir "$unavailable_source"
      tar -xzf "$upgrade_old_archive" -C "$unavailable_source" \
        || fail "upgrade golden: unavailable fixture extraction failed"
      nix run "$checkout#install" -- \
        --root "$unavailable_project" --project-id upgrade-golden-unavailable --name upgrade-golden-unavailable \
        --nixfied-url "path:$unavailable_source" >/dev/null \
        || fail "upgrade golden: unavailable fixture install failed"
      nix flake lock "$unavailable_project" >/dev/null \
        || fail "upgrade golden: unavailable fixture lock failed"
      jq '.nodes[.nodes[.root].inputs.nixfied].locked.narHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="' \
        "$unavailable_project/flake.lock" >"$unavailable_project/flake.lock.invalid" \
        || fail "upgrade golden: unavailable lock fixture could not be prepared"
      mv "$unavailable_project/flake.lock.invalid" "$unavailable_project/flake.lock"
      rm -rf "$unavailable_source"
      nix run "$checkout#upgrade" -- \
        --root "$unavailable_project" --nixfied-url "$new_path_pin" \
        >"$unavailable_project/stdout" 2>"$unavailable_project/stderr" \
        || fail "upgrade golden: unavailable source prevented a valid upgrade"
      grep -Fq 'documentation diff unavailable: old source materialization failed' "$unavailable_project/stderr" \
        || fail "upgrade golden: unavailable source warning was missing"
      grep -Fq -- '--- DOCUMENTATION DIFF UNAVAILABLE ---' "$unavailable_project/stdout" \
        || fail "upgrade golden: unavailable source was not reported"
      ! grep -Fq -- 'NO CHECKED-IN DOCUMENTATION CHANGED' "$unavailable_project/stdout" \
        || fail "upgrade golden: unavailable source was reported as empty"
      grep -Fq 'candidate verification: passed (model preflight)' "$unavailable_project/stderr" \
        || fail "upgrade golden: unavailable source incorrectly failed candidate preflight"
      ! grep -Fq '  rev:' "$unavailable_project/stderr" \
        || fail "upgrade golden: unavailable path identity fabricated a revision"
      rm -rf "$unavailable_project" "$source_root" "$git_root"
      printf '  upgrade_versioned: ok\n' >&2
    }

    upgrade_versioned_tests

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

    task_output_state="$st/task-output-state"
    task_output_stdout="$st/task-output.stdout"
    task_output_stderr="$st/task-output.stderr"
    task_output_expected="$st/task-output.expected"
    printf 'ok\n' >"$task_output_expected"
    ( cd "$wk" && NIXFIED_STATE_DIR="$task_output_state" nix run "$project#smoke" -- --timeout-ms 60000 --output task-output ) \
      >"$task_output_stdout" 2>"$task_output_stderr" \
      || fail "adoption: generated smoke task-output verb failed"
    cmp -s "$task_output_expected" "$task_output_stdout" \
      || fail "adoption: generated smoke task-output stdout was not exact"
    ! grep -Fq '"task"' "$task_output_stdout" \
      || fail "adoption: generated smoke task-output emitted runtime metadata"

    composite_task_output_state="$st/composite-task-output-state"
    composite_task_output_stdout="$st/composite-task-output.stdout"
    composite_task_output_stderr="$st/composite-task-output.stderr"
    composite_task_output_code=0
    if ( cd "$wk" && NIXFIED_STATE_DIR="$composite_task_output_state" \
         nix run "$project#composite-smoke" -- --timeout-ms 60000 --output task-output ) \
         >"$composite_task_output_stdout" 2>"$composite_task_output_stderr"; then
      :
    else
      composite_task_output_code=$?
    fi
    [ "$composite_task_output_code" -eq 37 ] \
      || fail "adoption: generated composite task-output exited $composite_task_output_code, want 37"
    [ ! -e "$composite_task_output_state" ] \
      || fail "adoption: generated composite task-output materialized runtime state"
    [ ! -s "$composite_task_output_stdout" ] \
      || fail "adoption: generated composite task-output wrote stdout"
    grep -Fq "TASK_SELECTION_INVALID" "$composite_task_output_stderr" \
      || fail "adoption: generated composite task-output omitted its typed rejection"

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
