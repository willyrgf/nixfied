{
  pkgs,
  bundleFile,
}:
pkgs.writeShellScript "nixfied-introspection-runtime" ''
  set -euo pipefail

  BUNDLE_FILE=${pkgs.lib.escapeShellArg (builtins.toString bundleFile)}
  JQ=${pkgs.lib.escapeShellArg "${pkgs.jq}/bin/jq"}

  fail() {
    printf 'ERROR: %s\n' "$1" >&2
    exit 2
  }

  print_help() {
    cat <<'EOF'
Usage: nix run .#introspect -- [query] [--json] [--kind <kind>] [--why <target>] [--reverse <target>] [--resolution-only|--execution|--closure]

Examples:
  nix run .#introspect -- check
  nix run .#introspect -- app:check --json
  nix run .#introspect -- task:task.check
  nix run .#introspect -- workflow:workflow.ci.full
  nix run .#introspect -- service:postgres
  nix run .#introspect -- service-set:default
  nix run .#introspect -- check --why package:nix-checks
  nix run .#introspect -- reverse package:nix-checks
EOF
  }

  is_valid_kind() {
    case "''${1:-}" in
      app|package|service|service-set|task|workflow)
        return 0
        ;;
      *)
        return 1
        ;;
    esac
  }

  emit_query_lines() {
    local raw="$1"
    local source_kind="$2"
    local selector="$3"
    printf 'INFO: query.raw=%s\n' "$raw"
    printf 'INFO: query.source_kind=%s\n' "$source_kind"
    printf 'INFO: query.selector=%s\n' "$selector"
  }

  emit_diagnostics_lines() {
    "$JQ" -r '.diagnosticsHumanLines[]' "$BUNDLE_FILE"
  }

  trim_json() {
    case "$VIEW_MODE" in
      resolution)
        "$JQ" '{query: .query, resolved: .resolved, resolution: .resolution, diagnostics: .diagnostics}'
        ;;
      execution)
        "$JQ" '{query: .query, resolved: .resolved, execution: .execution, diagnostics: .diagnostics}'
        ;;
      closure)
        "$JQ" '{query: .query, resolved: .resolved, closure: .closure, reverse: .reverse, diagnostics: .diagnostics}'
        ;;
      *)
        "$JQ" '.'
        ;;
    esac
  }

  resolve_query_entry() {
    local token="$1"
    local kind_filter="$2"
    local prefix=""
    local entry_json="null"
    local ambiguous_json="null"
    local ambiguous_labels=""
    local scope="any"

    prefix="''${token%%:*}"
    if [ "$prefix" != "$token" ] && is_valid_kind "$prefix"; then
      if [ -n "$kind_filter" ] && [ "$prefix" != "$kind_filter" ]; then
        fail "query '$token' does not match --kind $kind_filter"
      fi

      entry_json="$("$JQ" -c --arg token "$token" '.resolutionIndex.explicit[$token] // null' "$BUNDLE_FILE")"
      if [ "$entry_json" = "null" ]; then
        fail "unable to resolve '$token'"
      fi
      printf '%s' "$entry_json"
      return 0
    fi

    if [ -n "$kind_filter" ]; then
      scope="$kind_filter"
    fi

    ambiguous_json="$("$JQ" -c --arg scope "$scope" --arg token "$token" '
      if $scope == "any" then
        (.resolutionIndex.ambiguousBare.any[$token] // null)
      else
        (.resolutionIndex.ambiguousBare.byKind[$scope][$token] // null)
      end
    ' "$BUNDLE_FILE")"
    if [ "$ambiguous_json" != "null" ]; then
      ambiguous_labels="$(printf '%s' "$ambiguous_json" | "$JQ" -r 'join(", ")')"
      fail "ambiguous query '$token' matched: $ambiguous_labels"
    fi

    entry_json="$("$JQ" -c --arg scope "$scope" --arg token "$token" '
      if $scope == "any" then
        (.resolutionIndex.bare.any[$token] // null)
      else
        (.resolutionIndex.bare.byKind[$scope][$token] // null)
      end
    ' "$BUNDLE_FILE")"
    if [ "$entry_json" = "null" ]; then
      fail "unable to resolve '$token'"
    fi

    printf '%s' "$entry_json"
  }

  emit_query_json() {
    local raw="$1"
    local source_kind="$2"
    local selector="$3"
    local node_id="$4"

    "$JQ" -n \
      --slurpfile bundle "$BUNDLE_FILE" \
      --arg raw "$raw" \
      --arg sourceKind "$source_kind" \
      --arg selector "$selector" \
      --arg nodeId "$node_id" \
      --arg mode "$VIEW_MODE" '
      ($bundle[0].nodeViews[$nodeId].jsonByMode[$mode]) as $view
      | $view
      | .query = {
          raw: $raw,
          sourceKind: $sourceKind,
          selector: $selector
        }
    ' | trim_json | "$JQ" -S '.'
  }

  emit_why_json() {
    local raw="$1"
    local source_kind="$2"
    local selector="$3"
    local node_id="$4"
    local target_raw="$5"
    local target_source_kind="$6"
    local target_selector="$7"
    local target_node_id="$8"

    if [ "$VIEW_MODE" = "resolution" ] || [ "$VIEW_MODE" = "execution" ]; then
      emit_query_json "$raw" "$source_kind" "$selector" "$node_id"
      return 0
    fi

    "$JQ" -n \
      --slurpfile bundle "$BUNDLE_FILE" \
      --arg raw "$raw" \
      --arg sourceKind "$source_kind" \
      --arg selector "$selector" \
      --arg nodeId "$node_id" \
      --arg mode "$VIEW_MODE" \
      --arg targetRaw "$target_raw" \
      --arg targetSourceKind "$target_source_kind" \
      --arg targetSelector "$target_selector" \
      --arg targetNodeId "$target_node_id" '
      ($bundle[0].nodeViews[$nodeId].jsonByMode[$mode]) as $view
      | ($bundle[0].reverseViews[$targetNodeId]) as $reverseView
      | $view
      | .query = {
          raw: $raw,
          sourceKind: $sourceKind,
          selector: $selector
        }
      | .closure = {
          target: (
            $reverseView.target
            + {
              raw: $targetRaw,
              sourceKind: $targetSourceKind,
              selector: $targetSelector
            }
          ),
          summary: ($view.closure.summary // null),
          reasonChains: ($reverseView.reasonChainsBySource[$nodeId] // [])
        }
      | .reverse = null
    ' | trim_json | "$JQ" -S '.'
  }

  emit_reverse_json() {
    local target_raw="$1"
    local target_source_kind="$2"
    local target_selector="$3"
    local target_node_id="$4"

    "$JQ" -n \
      --slurpfile bundle "$BUNDLE_FILE" \
      --arg mode "$VIEW_MODE" \
      --arg targetRaw "$target_raw" \
      --arg targetSourceKind "$target_source_kind" \
      --arg targetSelector "$target_selector" \
      --arg targetNodeId "$target_node_id" '
      ($bundle[0].reverseViews[$targetNodeId].jsonByMode[$mode]) as $view
      | if ($view.reverse // null) == null then
          $view
        else
          $view
          | .reverse.target = (
              .reverse.target
              + {
                raw: $targetRaw,
                sourceKind: $targetSourceKind,
                selector: $targetSelector
              }
            )
        end
    ' | trim_json | "$JQ" -S '.'
  }

  emit_query_human() {
    local raw="$1"
    local source_kind="$2"
    local selector="$3"
    local node_id="$4"

    emit_query_lines "$raw" "$source_kind" "$selector"
    "$JQ" -r --arg nodeId "$node_id" --arg mode "$VIEW_MODE" '.nodeViews[$nodeId].humanByMode[$mode][]?' "$BUNDLE_FILE"
    emit_diagnostics_lines
  }

  emit_why_human() {
    local raw="$1"
    local source_kind="$2"
    local selector="$3"
    local node_id="$4"
    local target_node_id="$5"

    if [ "$VIEW_MODE" = "resolution" ] || [ "$VIEW_MODE" = "execution" ]; then
      emit_query_human "$raw" "$source_kind" "$selector" "$node_id"
      return 0
    fi

    emit_query_lines "$raw" "$source_kind" "$selector"
    "$JQ" -r --arg nodeId "$node_id" --arg mode "$VIEW_MODE" '.nodeViews[$nodeId].humanByMode[$mode][]?' "$BUNDLE_FILE"
    "$JQ" -r --arg targetNodeId "$target_node_id" --arg sourceNodeId "$node_id" '
      (.reverseViews[$targetNodeId].whyLinesBySource[$sourceNodeId] // [
        "WARN: no reason chains from " + $sourceNodeId + " to " + $targetNodeId
      ])[]
    ' "$BUNDLE_FILE"
    emit_diagnostics_lines
  }

  emit_reverse_human() {
    local target_node_id="$1"

    "$JQ" -r --arg targetNodeId "$target_node_id" --arg mode "$VIEW_MODE" '.reverseViews[$targetNodeId].humanByMode[$mode][]?' "$BUNDLE_FILE"
    emit_diagnostics_lines
  }

  JSON_OUTPUT=0
  KIND_FILTER=""
  WHY_TARGET=""
  REVERSE_TARGET=""
  VIEW_MODE="default"
  POSITIONALS=()
  VIEW_FLAG_COUNT=0

  while [ "$#" -gt 0 ]; do
    case "$1" in
      -h|--help)
        print_help
        exit 0
        ;;
      --json)
        JSON_OUTPUT=1
        shift
        ;;
      --kind)
        shift
        if [ "$#" -lt 1 ]; then
          fail "--kind requires a value"
        fi
        KIND_FILTER="$1"
        shift
        ;;
      --why)
        shift
        if [ "$#" -lt 1 ]; then
          fail "--why requires a value"
        fi
        WHY_TARGET="$1"
        shift
        ;;
      --reverse)
        shift
        if [ "$#" -lt 1 ]; then
          fail "--reverse requires a value"
        fi
        REVERSE_TARGET="$1"
        shift
        ;;
      --resolution-only)
        VIEW_MODE="resolution"
        VIEW_FLAG_COUNT=$((VIEW_FLAG_COUNT + 1))
        shift
        ;;
      --execution)
        VIEW_MODE="execution"
        VIEW_FLAG_COUNT=$((VIEW_FLAG_COUNT + 1))
        shift
        ;;
      --closure)
        VIEW_MODE="closure"
        VIEW_FLAG_COUNT=$((VIEW_FLAG_COUNT + 1))
        shift
        ;;
      *)
        POSITIONALS+=("$1")
        shift
        ;;
    esac
  done

  if [ -n "$KIND_FILTER" ] && ! is_valid_kind "$KIND_FILTER"; then
    fail "--kind must be one of: app, package, service, service-set, task, workflow"
  fi

  if [ "$VIEW_FLAG_COUNT" -gt 1 ]; then
    fail "only one of --resolution-only, --execution, or --closure may be used"
  fi

  if [ -n "$WHY_TARGET" ] && [ -n "$REVERSE_TARGET" ]; then
    fail "--why and --reverse cannot be used together"
  fi

  if [ "''${#POSITIONALS[@]}" -gt 0 ] && [ "''${POSITIONALS[0]}" = "reverse" ]; then
    if [ -n "$REVERSE_TARGET" ]; then
      fail "reverse target was provided twice"
    fi
    if [ "''${#POSITIONALS[@]}" -ne 2 ]; then
      fail "reverse mode requires exactly one target"
    fi
    REVERSE_TARGET="''${POSITIONALS[1]}"
    POSITIONALS=()
  fi

  if [ "''${#POSITIONALS[@]}" -gt 1 ]; then
    fail "expected at most one query token"
  fi

  QUERY="''${POSITIONALS[0]:-}"

  if [ -n "$WHY_TARGET" ] && [ -z "$QUERY" ]; then
    fail "--why requires a query token"
  fi

  if [ -n "$REVERSE_TARGET" ] && [ -n "$QUERY" ]; then
    fail "reverse mode does not accept a separate query token"
  fi

  if [ -z "$QUERY" ] && [ -z "$REVERSE_TARGET" ]; then
    fail "expected a query token or reverse target"
  fi

  if [ -n "$REVERSE_TARGET" ]; then
    TARGET_ENTRY_JSON="$(resolve_query_entry "$REVERSE_TARGET" "")"
    IFS=$'\t' read -r TARGET_NODE_ID TARGET_SOURCE_KIND TARGET_SELECTOR <<<"$(printf '%s' "$TARGET_ENTRY_JSON" | "$JQ" -r '[.nodeId, .sourceKind, .selector] | @tsv')"

    if [ "$JSON_OUTPUT" -eq 1 ]; then
      emit_reverse_json "$REVERSE_TARGET" "$TARGET_SOURCE_KIND" "$TARGET_SELECTOR" "$TARGET_NODE_ID"
    else
      emit_reverse_human "$TARGET_NODE_ID"
    fi
    exit 0
  fi

  ENTRY_JSON="$(resolve_query_entry "$QUERY" "$KIND_FILTER")"
  IFS=$'\t' read -r NODE_ID SOURCE_KIND SELECTOR <<<"$(printf '%s' "$ENTRY_JSON" | "$JQ" -r '[.nodeId, .sourceKind, .selector] | @tsv')"

  if [ -n "$WHY_TARGET" ]; then
    TARGET_ENTRY_JSON="$(resolve_query_entry "$WHY_TARGET" "")"
    IFS=$'\t' read -r TARGET_NODE_ID TARGET_SOURCE_KIND TARGET_SELECTOR <<<"$(printf '%s' "$TARGET_ENTRY_JSON" | "$JQ" -r '[.nodeId, .sourceKind, .selector] | @tsv')"
    if [ "$JSON_OUTPUT" -eq 1 ]; then
      emit_why_json "$QUERY" "$SOURCE_KIND" "$SELECTOR" "$NODE_ID" "$WHY_TARGET" "$TARGET_SOURCE_KIND" "$TARGET_SELECTOR" "$TARGET_NODE_ID"
    else
      emit_why_human "$QUERY" "$SOURCE_KIND" "$SELECTOR" "$NODE_ID" "$TARGET_NODE_ID"
    fi
    exit 0
  fi

  if [ "$JSON_OUTPUT" -eq 1 ]; then
    emit_query_json "$QUERY" "$SOURCE_KIND" "$SELECTOR" "$NODE_ID"
  else
    emit_query_human "$QUERY" "$SOURCE_KIND" "$SELECTOR" "$NODE_ID"
  fi
''
