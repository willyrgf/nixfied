{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  vendoredMetadata = import ../../nixfied/framework/install/internal/vendored-metadata.nix {
    inherit pkgs;
  };
in
pkgs.runCommand "vendored-metadata-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  source ${vendoredMetadata}

  export GIT="${pkgs.git}/bin/git"
  export SOURCE_GIT_ROOT="$TMPDIR/no-git-source"
  mkdir -p "$SOURCE_GIT_ROOT"

  export FRAMEWORK_REVISION="bbbbbbbbbbbb"
  export PREV_FRAMEWORK_REVISION="aaaaaaaaaaaa"
  write_vendored_metadata "$TMPDIR/upgrade.txt"
  require_contains "$TMPDIR/upgrade.txt" "Framework source revision (install/upgrade):"
  require_contains "$TMPDIR/upgrade.txt" "- bbbbbbbbbbbb"
  require_contains "$TMPDIR/upgrade.txt" "Changes since previous vendored revision (aaaaaaaaaaaa..bbbbbbbbbbbb):"
  require_contains "$TMPDIR/upgrade.txt" "- exact git history unavailable from packaged source"
  require_contains "$TMPDIR/upgrade.txt" "- compare: https://github.com/willyrgf/nixfied/compare/aaaaaaaaaaaa...bbbbbbbbbbbb"

  export FRAMEWORK_REVISION="cccccccccccc"
  export PREV_FRAMEWORK_REVISION="cccccccccccc"
  write_vendored_metadata "$TMPDIR/no-delta.txt"
  require_contains "$TMPDIR/no-delta.txt" "Changes since previous vendored revision (cccccccccccc..cccccccccccc):"
  require_contains "$TMPDIR/no-delta.txt" "- none (previous vendored revision already matches current framework revision)"

  export FRAMEWORK_REVISION="dddddddddddd"
  unset PREV_FRAMEWORK_REVISION
  write_vendored_metadata "$TMPDIR/install.txt"
  require_contains "$TMPDIR/install.txt" "Recent framework changes:"
  require_contains "$TMPDIR/install.txt" "- exact git history unavailable from packaged source"

  echo "OK: vendored metadata helper contract is validated" > "$out"
''
