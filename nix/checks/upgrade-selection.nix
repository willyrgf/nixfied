# Runs outside the build sandbox: exercise native Nix lock resolution against a
# local Git repository through the packaged upgrader, without network access.
{ pkgs }:
let
  upgrade = import ../install/upgrade.nix { inherit pkgs; };
in
pkgs.writeShellApplication {
  name = "nixfied-upgrade-selection-tests";
  runtimeInputs = [
    pkgs.coreutils
    pkgs.diffutils
    pkgs.git
    pkgs.gnugrep
    pkgs.jq
    pkgs.nix
  ];
  text = ''
    TMPDIR="$(cd "''${TMPDIR:-/tmp}" && pwd -P)"
    export TMPDIR GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
    work=$(mktemp -d)
    cleanup() {
      local test_status=$?
      if [[ "$test_status" -ne 0 && -f "$work/stderr" ]]; then cat "$work/stderr" >&2; fi
      rm -rf "$work"
      return "$test_status"
    }
    trap cleanup EXIT
    source="$work/source"
    mkdir "$source"
    git -c init.defaultBranch=main init -q "$source"
    git -C "$source" config user.name 'Upgrade fixture'
    git -C "$source" config user.email 'upgrade@example.invalid'
    cat >"$source/flake.nix" <<'FLAKE'
    { outputs = _: throw "upgrade selection must not evaluate source outputs"; }
    FLAKE
    printf 'first revision\n' >"$source/README.md"
    git -C "$source" add .
    git -C "$source" commit -qm first
    old_rev=$(git -C "$source" rev-parse HEAD)
    tracking_url="git+file://$source?ref=main&shallow=1"
    pinned_url="git+file://$source?ref=main&rev=$old_rev&shallow=1"

    for selection in tracking pinned; do
      project="$work/$selection"
      mkdir "$project"
      url="$tracking_url"
      if [[ "$selection" == pinned ]]; then url="$pinned_url"; fi
      cat >"$project/flake.nix" <<FLAKE
    {
      inputs.nixfied.url = "$url";
      outputs = _: throw "upgrade selection must not evaluate project outputs";
    }
    FLAKE
      printf '{ projectOwned = true; }\n' >"$project/nixfied.nix"
      nix flake lock "$project" >/dev/null
    done

    snapshot() {
      sha256sum "$project/flake.nix" "$project/flake.lock" "$project/nixfied.nix"
    }
    run_upgrade() {
      ${upgrade}/bin/nixfied-upgrade --root "$project" "$@" >"$work/stdout" 2>"$work/stderr"
    }
    assert_unchanged() {
      [[ "$(snapshot)" == "$before" ]]
      grep -Fxq 'candidate manifest evaluation: not run (--plan)' "$work/stderr"
    }
    assert_absent() {
      if grep -Fq -- "$1" "$work/stderr"; then
        printf 'unexpected report: %s\n' "$1" >&2
        return 1
      fi
    }
    cat >"$work/changed.expected" <<'DIFF'
    --- BEGIN NIXFIED DOCUMENTATION DIFF ---
    --- old/README.md
    +++ new/README.md
    @@ -1 +1 @@
    -first revision
    +second revision
    --- END NIXFIED DOCUMENTATION DIFF ---
    DIFF
    printf '%s\n' '--- BEGIN NIXFIED DOCUMENTATION DIFF ---' \
      '--- NO CHECKED-IN DOCUMENTATION CHANGED ---' \
      '--- END NIXFIED DOCUMENTATION DIFF ---' >"$work/unchanged.expected"

    printf 'second revision\n' >"$source/README.md"
    git -C "$source" commit -qam second
    new_rev=$(git -C "$source" rev-parse HEAD)

    # A tracking URL advances without an override; a locked revision alone must
    # never be mistaken for an explicitly pinned input URL.
    project="$work/tracking"
    before=$(snapshot)
    run_upgrade --plan
    assert_unchanged
    cmp "$work/changed.expected" "$work/stdout"
    grep -Fxq "  original: $tracking_url" "$work/stderr"
    grep -Fxq "  rev: $new_rev" "$work/stderr"
    assert_absent 'commit-pinned input URL'

    # An explicit revision stays fixed, visibly reports the complete reference,
    # and suggests removing only the revision, preserving the named branch.
    project="$work/pinned"
    before=$(snapshot)
    run_upgrade --plan
    assert_unchanged
    cmp "$work/unchanged.expected" "$work/stdout"
    grep -Fxq "  original: $pinned_url" "$work/stderr"
    grep -Fxq "  rev: $old_rev" "$work/stderr"
    assert_absent "  rev: $new_rev"
    grep -Fq 'commit-pinned input URL; refreshing keeps the requested revision' "$work/stderr"
    printf -v quoted_url '%q' "$tracking_url"
    grep -Fq -- "--nixfied-url $quoted_url (review with --plan; any named ref is preserved)" "$work/stderr"

    # Preview and apply the suggested one-time selection; no project semantics
    # or output evaluation are needed to select a tracking URL.
    run_upgrade --nixfied-url "$tracking_url" --plan
    assert_unchanged
    cmp "$work/changed.expected" "$work/stdout"
    grep -Fxq "  original: $tracking_url" "$work/stderr"
    assert_absent 'commit-pinned input URL'
    declaration_before=$(sha256sum "$project/nixfied.nix")
    run_upgrade --nixfied-url "$tracking_url" --force
    cmp "$work/changed.expected" "$work/stdout"
    grep -Fxq 'candidate manifest evaluation: skipped (--force)' "$work/stderr"
    grep -Fq "inputs.nixfied.url = \"$tracking_url\";" "$project/flake.nix"
    [[ "$(sha256sum "$project/nixfied.nix")" == "$declaration_before" ]]
    jq -e --arg rev "$new_rev" '
      .nodes[.nodes[.root].inputs.nixfied]
      | .locked.rev == $rev and .original.ref == "main" and .original.rev == null
    ' "$project/flake.lock" >/dev/null

    # The next invocation needs no override and discovers the next commit.
    printf 'third revision\n' >"$source/README.md"
    git -C "$source" commit -qam third
    next_rev=$(git -C "$source" rev-parse HEAD)
    before=$(snapshot)
    run_upgrade --plan
    assert_unchanged
    grep -Fxq "  rev: $next_rev" "$work/stderr"
    grep -Fxq '+third revision' "$work/stdout"
    assert_absent 'commit-pinned input URL'
    echo 'upgrade selection: tracking, explicit pin, and one-time switch passed' >&2
  '';
}
