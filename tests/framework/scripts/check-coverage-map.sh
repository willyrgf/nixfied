#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
MAP="$ROOT/tests/framework/COVERAGE_MAP.txt"
FRAMEWORK_DIR="$ROOT/nixfied/.framework"

if [ ! -f "$MAP" ]; then
  echo "ERROR: coverage map missing path=$MAP" >&2
  exit 1
fi

if [ ! -d "$FRAMEWORK_DIR" ]; then
  echo "ERROR: framework directory missing path=$FRAMEWORK_DIR" >&2
  exit 1
fi

MISSING=0
COUNT=0

while IFS= read -r file; do
  rel="${file#$ROOT/}"
  COUNT=$((COUNT + 1))
  if ! grep -Fq "\`$rel\`" "$MAP"; then
    echo "ERROR: coverage map missing entry file=$rel" >&2
    MISSING=$((MISSING + 1))
  fi
done < <(find "$FRAMEWORK_DIR" -type f -name '*.nix' | sort)

if [ "$MISSING" -ne 0 ]; then
  echo "ERROR: coverage map incomplete missing=$MISSING total=$COUNT" >&2
  exit 1
fi

echo "OK: coverage map complete total=$COUNT"
