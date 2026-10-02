# Nixfied upgrade surface, packaged as a shell application.
# The repin script is embedded here (literal `${...}` is escaped as
# `''${...}` for the Nix indented string); `nix run .#upgrade` runs it.
{ pkgs }:
let
  structure = import ../meta/default.nix { inherit (pkgs) lib; };
  syntax = import ../meta/command-default.nix { inherit (pkgs) lib; };
  projection = import ../meta/syntax-project.nix { inherit (pkgs) lib; inherit structure; } syntax;
  upgradeFiles = import ./upgrade-files.nix { inherit pkgs; };
in
pkgs.writeShellApplication {
  name = "nixfied-upgrade";
  runtimeInputs = [
    pkgs.coreutils
    pkgs.diffutils
    pkgs.findutils
    pkgs.gnused
    pkgs.jq
    pkgs.nix
    upgradeFiles
  ];
  text = ''
    set -euo pipefail
    export LC_ALL=C

    # Nixfied upgrade surface.
    #
    # Ownership boundary:
    #   - Nixfied owns the flake input pin and the import/compile wiring.
    #   - The project owns every semantic declaration in nixfied.nix.
    # This command therefore only ever rewrites the `nixfied.url` input pin and
    # refreshes the lock entry for that input. It never creates, edits, or deletes
    # nixfied.nix, and it makes no compatibility promise for already-compiled manifests.

    ${projection.shell "upgrade"}
    root="$UPGRADE_ROOT_INITIAL"
    nixfied_url="$UPGRADE_NIXFIED_URL_INITIAL"
    update_lock=$((1 - UPGRADE_NO_LOCK_INITIAL))
    plan="$UPGRADE_PLAN_INITIAL"
    force="$UPGRADE_FORCE_INITIAL"

    usage() {
      echo "$UPGRADE_HELP"
    }

    take_value() {
      local flag="$1"
      shift
      if [[ $# -eq 0 || -z "$1" || "$1" == --* ]]; then
        echo "missing $flag value" >&2
        exit 2
      fi
      printf '%s' "$1"
    }

    while [[ $# -gt 0 ]]; do
      case "$1" in
        "$UPGRADE_ROOT")
          root="$(take_value "$1" "''${2-}")"
          shift 2
          ;;
        "$UPGRADE_NIXFIED_URL")
          nixfied_url="$(take_value "$1" "''${2-}")"
          shift 2
          ;;
        "$UPGRADE_PLAN")
          plan=1
          shift
          ;;
        "$UPGRADE_FORCE")
          force=1
          shift
          ;;
        "$UPGRADE_NO_LOCK")
          update_lock=0
          shift
          ;;
        "$HELP_SHORT" | "$HELP_LONG")
          usage
          exit 0
          ;;
        *)
          echo "unknown upgrade argument: $1" >&2
          usage >&2
          exit 2
          ;;
      esac
    done

    mode=checked
    if [[ "$plan" -eq 1 ]]; then
      mode=plan
    elif [[ "$force" -eq 1 ]]; then
      mode=forced
    fi

    flake="$root/flake.nix"

    if [[ ! -e "$flake" ]]; then
      {
        echo "no Nixfied flake.nix to upgrade in $root"
        echo "No files were changed."
        echo "Run 'nixfied install' first to scaffold a project."
      } >&2
      exit 3
    fi

    root="$(realpath "$root")"
    flake="$root/flake.nix"
    flake_eval_path="$(realpath "$flake")"
    lock="$root/flake.lock"
    project_file="$root/nixfied.nix"

    has_nixfied_input_pin() {
      local verdict
      if ! verdict="$(
        NIXFIED_UPGRADE_FLAKE="$flake_eval_path" nix eval --impure --expr '
          let
            flake = import (builtins.getEnv "NIXFIED_UPGRADE_FLAKE");
          in
            flake ? inputs && flake.inputs ? nixfied && flake.inputs.nixfied ? url
        ' 2>/dev/null
      )"; then
        return 1
      fi
      [[ "$verdict" == "true" ]]
    }

    if ! has_nixfied_input_pin; then
      {
        echo "flake.nix in $root has no Nixfied input pin to upgrade"
        echo "No files were changed."
        echo "Expected an 'inputs.nixfied.url = \"...\";' assignment or an 'inputs.nixfied = { url = \"...\"; ... };' block."
      } >&2
      exit 3
    fi

    file_identity() {
      local path="$1"
      if [[ -f "$path" ]]; then
        sha256sum -- "$path" | cut -d ' ' -f1
      else
        printf '%s' absent
      fi
    }

    status_break() {
      printf '\n' >&2
    }

    flake_before="$(file_identity "$flake")"
    lock_before="$(file_identity "$lock")"
    project_before="$(file_identity "$project_file")"
    if [[ "$update_lock" -eq 1 && "$lock_before" == absent ]]; then
      echo "flake.lock is required for a locked upgrade because it identifies the old Nixfied source" >&2
      echo "No files were changed. Use --no-lock for the mechanical URL-only mode." >&2
      exit 3
    fi

    work="$(mktemp -d)"
    cleanup() {
      rm -rf -- "$work"
    }
    trap cleanup EXIT

    staged_flake=""
    if [[ -n "$nixfied_url" ]]; then
      staged_flake="$work/flake.nix"
      nixfied-upgrade-files rewrite "$flake" "$staged_flake" "$nixfied_url"
    fi

    assert_unchanged() {
      if [[ "$(file_identity "$flake")" != "$flake_before" \
        || "$(file_identity "$lock")" != "$lock_before" \
        || "$(file_identity "$project_file")" != "$project_before" ]]; then
        echo "upgrade aborted: flake.nix, flake.lock, or nixfied.nix changed while the candidate was being prepared" >&2
        echo "upgrade not applied; no project files were changed by this command" >&2
        exit 6
      fi
    }

    # Forward catchable interruption to the one owner of file application and
    # rollback. A second wait collects its result after an interrupted wait.
    apply_files() {
      local candidate_flake="$1" candidate_lock="$2" apply_pid apply_status=0 wait_interrupted=0
      nixfied-upgrade-files apply "$root" "$flake_before" "$lock_before" "$project_before" \
        "$candidate_flake" "$candidate_lock" &
      apply_pid=$!
      trap 'wait_interrupted=1; kill -TERM "$apply_pid" 2>/dev/null || true' INT TERM HUP
      while true; do
        wait_interrupted=0
        apply_status=0
        wait "$apply_pid" || apply_status=$?
        [[ "$wait_interrupted" -eq 1 ]] || break
      done
      trap - INT TERM HUP
      return "$apply_status"
    }

    if [[ "$update_lock" -eq 0 ]]; then
      echo "Nixfied upgrade inspection" >&2
      status_break
      echo "documentation diff: skipped (--no-lock; no candidate lock was produced)" >&2
      echo "candidate verification: skipped (--no-lock)" >&2
      if [[ -n "$nixfied_url" ]]; then
        if cmp -s "$staged_flake" "$flake"; then
          echo "unchanged: flake.nix" >&2
          if [[ "$plan" -eq 1 ]]; then
            echo "upgrade applied: no (--plan)" >&2
          else
            echo "upgrade applied: no (flake.nix already matched)" >&2
          fi
        elif [[ "$plan" -eq 1 ]]; then
          echo "plan: would rewrite flake.nix nixfied.url to $nixfied_url" >&2
          echo "upgrade applied: no (--plan)" >&2
        else
          assert_unchanged
          apply_files "$staged_flake" ""
          echo "upgrade applied: yes" >&2
          echo "changed: flake.nix (nixfied.url -> $nixfied_url)" >&2
        fi
      else
        echo "Nothing to upgrade in $root" >&2
        echo "Pass --nixfied-url to repin the input, or drop --no-lock to refresh the lock." >&2
        echo "upgrade applied: no (--no-lock; no URL supplied)" >&2
      fi
      if [[ -n "$project_file" && -f "$project_file" ]]; then
        echo "preserved: nixfied.nix (project-owned)" >&2
      fi
      if [[ "$plan" -eq 1 ]]; then
        echo "plan: no project files changed" >&2
      fi
      exit 0
    fi

    candidate_lock="$work/flake.lock"
    update_command=(nix flake update nixfied --flake "$root" --output-lock-file "$candidate_lock")
    if [[ -n "$nixfied_url" ]]; then
      update_command+=(--override-input nixfied "$nixfied_url")
    fi

    echo "Nixfied upgrade candidate" >&2
    echo "candidate selection: project input unless --nixfied-url is supplied; the supplying flake selects the upgrade tool" >&2
    if ! "''${update_command[@]}" >/dev/null; then
      status_break
      echo "candidate lock resolution failed" >&2
      echo "upgrade applied: no" >&2
      echo "upgrade not applied; no project files were changed" >&2
      exit 4
    fi
    status_break

    lock_node_json() {
      local lock_path="$1"
      jq -c '
        .nodes[.root].inputs.nixfied as $nixfied
        | .nodes[$nixfied] // empty
      ' "$lock_path"
    }

    # --override-input retains the project's old `original` in Nix's lock.
    # Bind the candidate to the URL that will actually be written, without
    # changing any resolved source identities or the other input nodes.
    if [[ -n "$nixfied_url" ]]; then
      if ! requested_original="$(NIXFIED_UPGRADE_REFERENCE="$nixfied_url" nix eval --json --impure --expr '
        builtins.parseFlakeRef (builtins.getEnv "NIXFIED_UPGRADE_REFERENCE")
      ')" || ! jq --argjson original "$requested_original" '
        .nodes[.root].inputs.nixfied as $nixfied
        | .nodes[$nixfied].original = $original
      ' "$candidate_lock" >"$work/candidate-bound.lock"; then
        echo "candidate lock reference binding failed" >&2
        echo "upgrade applied: no" >&2
        echo "upgrade not applied; no project files were changed" >&2
        exit 4
      fi
      mv "$work/candidate-bound.lock" "$candidate_lock"
    fi

    locked_reference() {
      NIXFIED_UPGRADE_SOURCE="$(lock_node_json "$1")" nix eval --raw --impure --expr '
        builtins.flakeRefToString (builtins.fromJSON (builtins.getEnv "NIXFIED_UPGRADE_SOURCE")).locked
      '
    }

    report_identity() {
      local label="$1"
      local lock_path="$2"
      local node_json selection_json
      if ! node_json="$(lock_node_json "$lock_path")" || [[ -z "$node_json" ]]; then
        echo "$label source: unavailable (nixfied lock node is missing)" >&2
        return 1
      fi
      if ! selection_json="$(NIXFIED_UPGRADE_SOURCE="$node_json" nix eval --json --impure --expr '
        let
          original = (builtins.fromJSON (builtins.getEnv "NIXFIED_UPGRADE_SOURCE")).original;
        in {
          reference = builtins.flakeRefToString original;
          withoutRevision = if original ? rev then
            builtins.flakeRefToString (builtins.removeAttrs original [ "rev" ])
          else null;
        }
      ')"; then
        echo "$label source: unavailable (original reference could not be rendered)" >&2
        return 1
      fi
      printf '%s source:\n' "$label" >&2
      printf '%s\n' "$node_json" | jq -r --argjson selection "$selection_json" '
        [
          "  type: " + (.locked.type // .original.type // "unknown"),
          "  original: " + $selection.reference,
          (if .locked.rev? != null then "  rev: " + .locked.rev else empty end),
          (if .locked.narHash? != null then "  narHash: " + .locked.narHash else empty end)
        ] | .[]
      ' >&2
      if [[ "$label" == candidate ]] && jq -e '.withoutRevision != null' <<<"$selection_json" >/dev/null; then
        echo "candidate selection: commit-pinned input URL; refreshing keeps the requested revision" >&2
        echo "for future updates, choose a repository or moving branch URL once; flake.lock keeps the exact revision" >&2
        printf '  without the commit pin: --nixfied-url %q (review with --plan; any named ref is preserved)\n' \
          "$(jq -r '.withoutRevision' <<<"$selection_json")" >&2
      fi
      return 0
    }

    old_source=""
    candidate_source=""
    report_identity "old" "$lock" || true
    status_break
    report_identity "candidate" "$candidate_lock" || true

    materialize_source() {
      local label="$1"
      local lock_path="$2"
      local error_path="$work/$label-archive.stderr"
      local archive_json source_path reference
      if [[ ! -f "$lock_path" ]]; then
        echo "documentation diff unavailable: $label source has no lock file" >&2
        return 1
      fi
      if ! reference="$(locked_reference "$lock_path" 2>"$error_path")" || \
        ! archive_json="$(nix flake archive --json --no-write-lock-file "$reference" 2>>"$error_path")"; then
        echo "documentation diff unavailable: $label source materialization failed" >&2
        [[ ! -s "$error_path" ]] || sed 's/^/  nix: /' "$error_path" >&2
        return 1
      fi
      if ! source_path="$(printf '%s\n' "$archive_json" | jq -er '.path // empty')"; then
        echo "documentation diff unavailable: $label source path was not present in nix flake archive output" >&2
        return 1
      fi
      if [[ ! -d "$source_path" ]]; then
        echo "documentation diff unavailable: $label source path is not a directory" >&2
        return 1
      fi
      printf '%s' "$source_path"
    }

    old_source="$(materialize_source old "$lock")" || old_source=""
    candidate_source="$(materialize_source candidate "$candidate_lock")" || candidate_source=""

    scope_paths() {
      local source="$1"
      if [[ -f "$source/README.md" ]]; then
        printf 'README.md\0'
      fi
      if [[ -d "$source/docs" ]]; then
        while IFS= read -r -d $'\0' file; do
          printf 'docs/%s\0' "''${file#"$source/docs/"}"
        done < <(find "$source/docs" -type f -print0)
      fi
    }

    emit_docs_diff() {
      local old="$1"
      local new="$2"
      local scope="$work/documentation-scope"
      local relative old_file new_file diff_status has_changes=0
      {
        scope_paths "$old"
        scope_paths "$new"
      } | sort -z -u >"$scope"
      while IFS= read -r -d $'\0' relative; do
        old_file="$old/$relative"
        new_file="$new/$relative"
        diff_status=0
        if [[ -f "$old_file" && -f "$new_file" ]]; then
          diff -u --label "old/$relative" --label "new/$relative" "$old_file" "$new_file" || diff_status=$?
        elif [[ -f "$old_file" ]]; then
          diff -u --label "old/$relative" --label "new/$relative" "$old_file" /dev/null || diff_status=$?
        else
          diff -u --label "old/$relative" --label "new/$relative" /dev/null "$new_file" || diff_status=$?
        fi
        if [[ "$diff_status" -gt 1 ]]; then
          echo "documentation diff failed while comparing $relative" >&2
          return 1
        fi
        if [[ "$diff_status" -eq 1 ]]; then
          has_changes=1
        fi
      done <"$scope"
      if [[ "$has_changes" -eq 0 ]]; then
        echo '--- NO CHECKED-IN DOCUMENTATION CHANGED ---'
      fi
    }

    status_break
    echo "documentation diff: emitted on stdout (README.md and docs/)" >&2
    echo "review tip: redirect stdout to a file (e.g. > nixfied-upgrade.diff); status stays on stderr" >&2
    status_break
    printf '%s\n' '--- BEGIN NIXFIED DOCUMENTATION DIFF ---'
    if [[ -n "$old_source" && -n "$candidate_source" ]]; then
      if ! emit_docs_diff "$old_source" "$candidate_source"; then
        echo '--- DOCUMENTATION DIFF UNAVAILABLE ---'
        echo "documentation diff unavailable: source comparison failed" >&2
      fi
    else
      echo '--- DOCUMENTATION DIFF UNAVAILABLE ---'
      echo "documentation diff unavailable: one or both locked Nixfied sources could not be materialized" >&2
    fi
    printf '%s\n' '--- END NIXFIED DOCUMENTATION DIFF ---'

    flake_changed=0
    lock_changed=0
    if [[ -n "$staged_flake" ]] && ! cmp -s "$staged_flake" "$flake"; then
      flake_changed=1
    fi
    if ! cmp -s "$candidate_lock" "$lock"; then
      lock_changed=1
    fi

    report_result() {
      local outcome="$1" evaluation action index
      local changes=("$flake_changed" "$lock_changed")
      local descriptions=("flake.nix" "flake.lock (nixfied input)")
      [[ "$flake_changed" -eq 0 ]] || descriptions[0]+=" (nixfied.url -> $nixfied_url)"
      case "$mode:$outcome" in
        plan:*) evaluation='not run (--plan)'; action='would change' ;;
        checked:rejected) evaluation=failed; action=blocked ;;
        checked:completed) evaluation=passed; action=changed ;;
        forced:completed) evaluation='skipped (--force)'; action=changed ;;
      esac
      status_break
      echo "candidate manifest evaluation: $evaluation" >&2
      status_break
      if [[ "$mode" == "plan" ]]; then
        echo "upgrade applied: no (--plan)" >&2
        echo "plan: no project files changed" >&2
      elif [[ "$outcome" == rejected ]]; then
        echo "upgrade applied: no (candidate manifest evaluation failed)" >&2
        echo "upgrade not applied; no project files were changed" >&2
      elif [[ "$flake_changed" -eq 0 && "$lock_changed" -eq 0 ]]; then
        echo "upgrade applied: no (project already matched candidate)" >&2
      else
        echo "upgrade applied: yes" >&2
        echo "upgraded Nixfied wiring in $root" >&2
      fi
      for index in "''${!changes[@]}"; do
        if [[ "''${changes[$index]}" -eq 0 ]]; then
          echo "unchanged: ''${descriptions[$index]}" >&2
        else
          echo "$action: ''${descriptions[$index]}" >&2
        fi
      done
      if [[ -f "$project_file" && "$(file_identity "$project_file")" == "$project_before" ]]; then
        echo "preserved: nixfied.nix (project-owned)" >&2
      elif [[ "$mode" != "plan" ]]; then
        echo "nixfied.nix changed concurrently; this command did not edit it" >&2
      fi
      status_break
      if [[ "$mode" == "plan" ]]; then
        echo "post-upgrade validation: not run (--plan)" >&2
      else
        echo "post-upgrade validation: not run" >&2
      fi
      echo "next:" >&2
      if [[ "$mode" == "plan" ]]; then
        echo "  rerun upgrade without --plan using the same supplying flake to attempt checked apply; the next invocation resolves upstream again" >&2
        echo "  or remove --plan and add --force to skip only manifest evaluation; transaction safeguards still apply" >&2
      elif [[ "$outcome" == rejected ]]; then
        echo "  edit project wiring/declarations and replan, or explicitly repin with --force and repair afterward" >&2
      fi
      echo "validation after apply and any project wiring/declaration repairs:" >&2
      echo "  nix build $root#manifest" >&2
      echo "  nix run $root#manifest-check" >&2
    }

    if [[ "$mode" == plan ]]; then
      assert_unchanged
      report_result completed
      exit 0
    fi

    if [[ "$mode" == checked ]]; then
      status_break
      if ! candidate_reference="$(locked_reference "$candidate_lock")" || \
        ! nix eval --no-write-lock-file --reference-lock-file "$candidate_lock" \
        --override-input nixfied "$candidate_reference" --raw \
        "$root#manifest.drvPath" >/dev/null; then
        report_result rejected
        exit 5
      fi
    fi

    if [[ -n "''${NIXFIED_UPGRADE_TEST_PAUSE_BEFORE_APPLY-}" ]]; then
      echo "test pause before apply" >&2
      sleep "''${NIXFIED_UPGRADE_TEST_PAUSE_BEFORE_APPLY}"
    fi
    assert_unchanged

    apply_files "$staged_flake" "$candidate_lock"
    report_result completed
  '';
}
