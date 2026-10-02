# Nixfied upgrade surface, packaged as a shell application.
# The repin script is embedded here (literal `${...}` is escaped as
# `''${...}` for the Nix indented string); `nix run .#upgrade` runs it.
{ pkgs }:
let
  structure = import ../meta/default.nix { inherit (pkgs) lib; };
  syntax = import ../meta/command-default.nix { inherit (pkgs) lib; };
  projection = import ../meta/syntax-project.nix { inherit (pkgs) lib; inherit structure; } syntax;
  atomicExchange = pkgs.stdenv.mkDerivation {
    name = "nixfied-atomic-exchange";
    src = ./atomic-exchange.c;
    dontUnpack = true;
    buildPhase = ''
      $CC -std=c11 -D_GNU_SOURCE -Wall -Wextra -Werror -o nixfied-atomic-exchange "$src"
    '';
    installPhase = ''
      mkdir -p "$out/bin"
      cp nixfied-atomic-exchange "$out/bin/"
    '';
  };
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
    pkgs.python3
    atomicExchange
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
    apply_temp=""
    cleanup() {
      [[ -z "$apply_temp" ]] || rm -f -- "$apply_temp"
      rm -rf -- "$work"
    }
    trap cleanup EXIT

    nix_escape() {
      local value="$1"
      value="''${value//\\/\\\\}"
      value="''${value//\"/\\\"}"
      printf '%s' "$value"
    }

    brace_delta() {
      local text="$1"
      local delta=0
      local i
      local ch
      for ((i = 0; i < ''${#text}; i++)); do
        ch="''${text:i:1}"
        case "$ch" in
          "{") delta=$((delta + 1)) ;;
          "}") delta=$((delta - 1)) ;;
        esac
      done
      printf '%s' "$delta"
    }

    is_direct_nixfied_url() {
      local stripped="$1"
      [[ "$stripped" == 'nixfied.url="'*'";'* || "$stripped" == 'inputs.nixfied.url="'*'";'* ]]
    }

    is_nixfied_input_block_start() {
      local stripped="$1"
      [[ "$stripped" == 'nixfied={'* || "$stripped" == 'inputs.nixfied={'* ]]
    }

    staged_flake=""
    stage_flake_rewrite() {
      [[ -n "$nixfied_url" ]] || return 0

      local nixfied_url_escaped rewritten matches in_nixfied_input_block
      local nixfied_input_block_depth line stripped indent
      nixfied_url_escaped="$(nix_escape "$nixfied_url")"
      rewritten="$work/flake.nix"
      matches=0
      in_nixfied_input_block=0
      nixfied_input_block_depth=0
      : >"$rewritten"
      while IFS= read -r line || [[ -n "$line" ]]; do
        # Compare on a whitespace-stripped form so we match the exact `nixfied.url`
        # assignment regardless of indentation/spacing. Attrset inputs are matched
        # as a scoped block and only their inner `url = "...";` line is rewritten.
        stripped="''${line//[[:space:]]/}"
        if [[ "$in_nixfied_input_block" -eq 0 ]] && is_direct_nixfied_url "$stripped"; then
          if [[ "$stripped" == 'inputs.nixfied.url="'* ]]; then
            indent="''${line%%inputs.nixfied.url*}"
            printf '%sinputs.nixfied.url = "%s";\n' "$indent" "$nixfied_url_escaped" >>"$rewritten"
          else
            indent="''${line%%nixfied.url*}"
            printf '%snixfied.url = "%s";\n' "$indent" "$nixfied_url_escaped" >>"$rewritten"
          fi
          matches=$((matches + 1))
        elif [[ "$in_nixfied_input_block" -eq 1 && "$stripped" == 'url="'*'";'* ]]; then
          indent="''${line%%url*}"
          printf '%surl = "%s";\n' "$indent" "$nixfied_url_escaped" >>"$rewritten"
          matches=$((matches + 1))
        else
          printf '%s\n' "$line" >>"$rewritten"
        fi

        if [[ "$in_nixfied_input_block" -eq 0 ]] && is_nixfied_input_block_start "$stripped"; then
          in_nixfied_input_block=1
          nixfied_input_block_depth="$(brace_delta "$line")"
          if [[ "$nixfied_input_block_depth" -le 0 ]]; then
            in_nixfied_input_block=0
          fi
        elif [[ "$in_nixfied_input_block" -eq 1 ]]; then
          nixfied_input_block_depth=$((nixfied_input_block_depth + $(brace_delta "$line")))
          if [[ "$nixfied_input_block_depth" -le 0 ]]; then
            in_nixfied_input_block=0
          fi
        fi
      done <"$flake"

      if [[ "$matches" -ne 1 ]]; then
        echo "expected exactly one nixfied input url assignment in flake.nix, found $matches" >&2
        echo "No files were changed." >&2
        echo "Refusing to guess which input pin to rewrite." >&2
        exit 3
      fi
      staged_flake="$rewritten"
    }

    assert_unchanged() {
      if [[ "$(file_identity "$flake")" != "$flake_before" \
        || "$(file_identity "$lock")" != "$lock_before" \
        || "$(file_identity "$project_file")" != "$project_before" ]]; then
        echo "upgrade aborted: flake.nix, flake.lock, or nixfied.nix changed while the candidate was being prepared" >&2
        echo "upgrade not applied; no project files were changed by this command" >&2
        exit 6
      fi
    }

    stage_flake_rewrite

    # Replace a regular file only if the content captured before candidate
    # preparation is still present. The exchange is atomic: a concurrent writer
    # is observed after the swap and the original directory entry is restored
    # before this function reports a conflict. Linux and Darwin provide the
    # needed exchange primitive under different names.
    atomic_replace() {
      python3 - "$1" "$2" "$3" <<'PY'
import hashlib
import os
import subprocess
import sys

staged, destination, expected = sys.argv[1:]

def digest(path):
    checksum = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            checksum.update(block)
    return checksum.hexdigest()

def fail(message, code):
    print("atomic replace: " + message, file=sys.stderr)
    raise SystemExit(code)

def exchange(old_path, new_path):
    result = subprocess.run(
        ["nixfied-atomic-exchange", old_path, new_path], capture_output=True, text=True
    )
    if result.returncode != 0:
        fail("atomic exchange failed: " + result.stderr.strip(), 4)

if not os.path.exists(staged):
    fail("staged file is missing", 4)
if os.path.lexists(destination):
    if not os.path.isfile(destination) or os.path.islink(destination):
        fail("destination is not a regular file", 4)
    if digest(destination) != expected:
        fail("destination changed concurrently", 3)
    exchange(staged, destination)
    if digest(staged) != expected:
        exchange(staged, destination)
        fail("destination changed concurrently", 3)
    try:
        os.unlink(staged)
    except OSError:
        pass
else:
    if expected != "absent":
        fail("destination disappeared concurrently", 3)
    try:
        os.link(staged, destination)
        os.unlink(staged)
    except FileExistsError:
        fail("destination appeared concurrently", 3)
PY
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
          install_flake="$flake.nixfied-upgrade.$$"
          apply_temp="$install_flake"
          cp -- "$staged_flake" "$install_flake"
          replace_status=0
          atomic_replace "$install_flake" "$flake" "$flake_before" || replace_status=$?
          rm -f -- "$install_flake"
          apply_temp=""
          if [[ "$replace_status" -eq 3 ]]; then
            echo "upgrade aborted: flake.nix changed concurrently; no project files were changed" >&2
            exit 6
          elif [[ "$replace_status" -ne 0 ]]; then
            echo "failed to apply flake.nix; no project files were changed" >&2
            exit 7
          fi
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

    candidate_lock_identity="$(file_identity "$candidate_lock")"
    candidate_flake_identity=""
    if [[ -n "$staged_flake" ]]; then
      candidate_flake_identity="$(file_identity "$staged_flake")"
    fi

    lock_backup="$work/flake.lock.before"
    flake_backup="$work/flake.nix.before"

    lock_apply_started=0
    flake_apply_started=0
    apply_in_progress=1
    rollback() {
      local rollback_failed=0 rollback_tmp replace_status current_identity
      if [[ "$flake_apply_started" -eq 1 ]]; then
        current_identity="$(file_identity "$flake")"
        if [[ "$current_identity" == "$flake_before" ]]; then
          flake_apply_started=0
        elif [[ "$current_identity" != "$candidate_flake_identity" ]]; then
          rollback_failed=1
        fi
      fi
      if [[ "$flake_apply_started" -eq 1 ]]; then
        rollback_tmp="$flake.nixfied-upgrade.rollback.$$"
        if ! cp -- "$flake_backup" "$rollback_tmp"; then
          rollback_failed=1
        else
          replace_status=0
          atomic_replace "$rollback_tmp" "$flake" "$candidate_flake_identity" || replace_status=$?
          rm -f -- "$rollback_tmp"
          [[ "$replace_status" -eq 0 ]] || rollback_failed=1
        fi
      fi
      if [[ "$lock_apply_started" -eq 1 ]]; then
        current_identity="$(file_identity "$lock")"
        if [[ "$current_identity" == "$lock_before" ]]; then
          lock_apply_started=0
        elif [[ "$current_identity" != "$candidate_lock_identity" ]]; then
          rollback_failed=1
        fi
      fi
      if [[ "$lock_apply_started" -eq 1 ]]; then
        rollback_tmp="$lock.nixfied-upgrade.rollback.$$"
        if ! cp -- "$lock_backup" "$rollback_tmp"; then
          rollback_failed=1
        else
          replace_status=0
          atomic_replace "$rollback_tmp" "$lock" "$candidate_lock_identity" || replace_status=$?
          rm -f -- "$rollback_tmp"
          [[ "$replace_status" -eq 0 ]] || rollback_failed=1
        fi
      fi
      return "$rollback_failed"
    }

    on_interrupt() {
      if [[ "$apply_in_progress" -eq 1 ]]; then
        if ! rollback; then
          echo "upgrade interrupted and rollback failed; inspect the project files" >&2
          exit 8
        fi
        echo "upgrade interrupted; candidate rolled back" >&2
      fi
      exit 130
    }
    trap on_interrupt INT TERM HUP

    if [[ "$lock_changed" -eq 1 ]]; then
      if ! cp -- "$lock" "$lock_backup"; then
        echo "failed to prepare flake.lock backup; no project files were changed" >&2
        exit 7
      fi
    fi
    if [[ "$flake_changed" -eq 1 ]]; then
      if ! cp -- "$flake" "$flake_backup"; then
        echo "failed to prepare flake.nix backup; no project files were changed" >&2
        exit 7
      fi
    fi

    if [[ "$lock_changed" -eq 1 ]]; then
      lock_install="$lock.nixfied-upgrade.$$"
      apply_temp="$lock_install"
      lock_apply_started=1
      if ! cp -- "$candidate_lock" "$lock_install"; then
        apply_temp=""
        if ! rollback; then
          echo "failed to prepare candidate flake.lock and rollback also failed; inspect the project files" >&2
          exit 8
        fi
        echo "failed to prepare candidate flake.lock; no project files were changed" >&2
        exit 7
      fi
      replace_status=0
      atomic_replace "$lock_install" "$lock" "$lock_before" || replace_status=$?
      rm -f -- "$lock_install"
      apply_temp=""
      if [[ "$replace_status" -eq 3 ]]; then
        echo "upgrade aborted: flake.lock changed concurrently; no project files were changed" >&2
        exit 6
      elif [[ "$replace_status" -ne 0 ]]; then
        if ! rollback; then
          echo "failed to apply candidate flake.lock and rollback also failed; inspect the project files" >&2
          exit 8
        fi
        echo "failed to apply candidate flake.lock; candidate was rolled back" >&2
        exit 7
      fi
      if [[ "$flake_changed" -eq 1 && "$(file_identity "$lock")" != "$candidate_lock_identity" ]]; then
        if ! rollback; then
          echo "flake.lock changed during the upgrade and rollback failed; inspect the project files" >&2
          exit 8
        fi
        echo "flake.lock changed during the upgrade; candidate was rolled back" >&2
        exit 6
      fi
    fi
    if [[ "$flake_changed" -eq 1 && -n "''${NIXFIED_UPGRADE_TEST_PAUSE_AFTER_LOCK-}" ]]; then
      echo "test pause after lock apply" >&2
      sleep "''${NIXFIED_UPGRADE_TEST_PAUSE_AFTER_LOCK}"
    fi
    if [[ "$flake_changed" -eq 1 ]]; then
      flake_install="$flake.nixfied-upgrade.$$"
      apply_temp="$flake_install"
      flake_apply_started=1
      if ! cp -- "$staged_flake" "$flake_install"; then
        apply_temp=""
        if ! rollback; then
          echo "failed to prepare candidate flake.nix and rollback also failed; inspect the project files" >&2
          exit 8
        fi
        echo "failed to prepare candidate flake.nix; candidate was rolled back" >&2
        exit 7
      fi
      replace_status=0
      atomic_replace "$flake_install" "$flake" "$flake_before" || replace_status=$?
      rm -f -- "$flake_install"
      apply_temp=""
      if [[ "$replace_status" -ne 0 ]]; then
        if ! rollback; then
          echo "failed to apply flake.nix and rollback also failed; inspect the project files" >&2
          exit 8
        fi
        if [[ "$replace_status" -eq 3 ]]; then
          echo "upgrade aborted: flake.nix changed concurrently; candidate was rolled back" >&2
          exit 6
        fi
        echo "failed to apply flake.nix; candidate was rolled back" >&2
        exit 7
      fi
    fi
    if [[ "$(file_identity "$lock")" != "$candidate_lock_identity" \
      || ( "$flake_changed" -eq 1 && "$(file_identity "$flake")" != "$candidate_flake_identity" ) ]]; then
      if ! rollback; then
        echo "project files changed during the upgrade and rollback failed; inspect the project files" >&2
        exit 8
      fi
      echo "project files changed during the upgrade; candidate was rolled back" >&2
      exit 6
    fi
    apply_in_progress=0
    trap - INT TERM HUP
    report_result completed
  '';
}
