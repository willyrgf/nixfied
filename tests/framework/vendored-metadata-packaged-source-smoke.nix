{ pkgs }:
let
  vendoredMetadataRuntime = import ../../nixfied/framework/install/internal/vendored-metadata.nix {
    inherit pkgs;
  };
in
pkgs.runCommand "vendored-metadata-packaged-source-smoke" { } ''
  set -euo pipefail

  GIT_BIN="${pkgs.git}/bin/git"
  PREV_REV="a53f03c95781cb9a540a0d61490471d49331528b"
  CURR_REV="0c2adefd02a9df85a5722c50f1d474e62c5e3a8f"
  METADATA_FILE="$TMPDIR/VENDORED.txt"
  SOURCE_GIT_ROOT="$TMPDIR/packaged-source"

  mkdir -p "$SOURCE_GIT_ROOT"
  source ${vendoredMetadataRuntime}

  GIT="$GIT_BIN"
  PREV_FRAMEWORK_REVISION="$PREV_REV"
  FRAMEWORK_REVISION="$CURR_REV"
  write_vendored_metadata "$METADATA_FILE"

  if ! ${pkgs.gnugrep}/bin/grep -Fq \
    "Changes since previous vendored revision ($PREV_REV..$CURR_REV):" \
    "$METADATA_FILE"; then
    echo "missing exact revision range header"
    cat "$METADATA_FILE"
    exit 1
  fi

  if ! ${pkgs.gnugrep}/bin/grep -Fq -- \
    "- exact git history unavailable from packaged source" \
    "$METADATA_FILE"; then
    echo "missing packaged-source fallback note"
    cat "$METADATA_FILE"
    exit 1
  fi

  if ! ${pkgs.gnugrep}/bin/grep -Fq -- \
    "- compare: https://github.com/willyrgf/nixfied/compare/$PREV_REV...$CURR_REV" \
    "$METADATA_FILE"; then
    echo "missing packaged-source compare hint"
    cat "$METADATA_FILE"
    exit 1
  fi

  echo "OK: packaged-source vendored metadata falls back to compare hints" > "$out"
''
