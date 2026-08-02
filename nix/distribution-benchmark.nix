# Repeatable, opt-in measurements for framework and adopter distribution cost.
# This is maintainer tooling, not another shipped framework product.
{
  pkgs,
  framework,
}:
pkgs.writeShellApplication {
  name = "nixfied-distribution-benchmark";
  runtimeInputs = [
    pkgs.nix
    pkgs.coreutils
    pkgs.findutils
    pkgs.gawk
    pkgs.gnugrep
    pkgs.bash
    pkgs.jq
    pkgs.time
  ];
  text = ''
    # shellcheck disable=SC2016
    usage() {
      cat >&2 <<'EOF'
    usage: nixfied-distribution-benchmark [--mode clean|warm] [--source-boundaries]

    The benchmark never garbage-collects or deletes store paths. Run it once in
    an isolated clean store and once against a warm store when comparing first
    use with reuse. --source-boundaries also runs the focused source/derivation
    invalidation check.
    EOF
    }

    mode="''${NIXFIED_BENCHMARK_MODE:-warm}"
    source_boundaries=0
    while (($# > 0)); do
      case "$1" in
        --mode)
          (($# >= 2)) || { usage; exit 2; }
          mode="$2"
          shift 2
          ;;
        --source-boundaries)
          source_boundaries=1
          shift
          ;;
        --help|-h)
          usage
          exit 0
          ;;
        *)
          usage
          exit 2
          ;;
      esac
    done
    case "$mode" in
      clean|warm) ;;
      *)
        echo "benchmark: --mode must be clean or warm" >&2
        exit 2
        ;;
    esac

    system="$(nix eval --impure --raw --expr builtins.currentSystem)"
    work="$(mktemp -d "''${TMPDIR:-/tmp}/nixfied-distribution-benchmark.XXXXXX")"
    trap 'rm -rf -- "$work"' EXIT
    framework="${framework}"
    project="$work/adopter"
    mkdir -p "$project"

    printf 'field\tvalue\n'
    printf 'system\t%s\n' "$system"
    printf 'nix_version\t%s\n' "$(nix --version)"
    printf 'mode\t%s\n' "$mode"
    printf 'framework\t%s\n' "$framework"
    printf 'source_boundaries\t%s\n' "$source_boundaries"
    printf 'measurement\tlabel\twall_seconds\tmaxrss_kb\tdownloaded_bytes\tplanned_paths\tnar_bytes\tclosure_bytes\tclosure_paths\trustc_logged\toutput\n'

    read_metric() {
      local key="$1"
      local file="$2"
      awk -F= -v key="$key" '$1 == key { value = $2 } END { if (value == "") value = "unknown"; print value }' "$file"
    }

    downloaded_metric() {
      local file="$1"
      local events
      events="$(sed -n 's/^@nix //p' "$file" | jq -R -s '
        [splits("\\n")[] | fromjson? | select(type == "object")]
        | map(select(has("downloadedBytes") or has("downloaded_bytes")))
        | map(.downloadedBytes // .downloaded_bytes)
        | { count: length, bytes: (add // 0) }
      ' 2>/dev/null || printf '{"count":0,"bytes":0}')"
      if [[ "$(jq -r '.count' <<<"$events")" == 0 ]]; then
        printf 'unreported'
      else
        jq -r '.bytes' <<<"$events"
      fi
    }

    rustc_metric() {
      local log="$1"
      if grep -Eq '(^|[^[:alnum:]_-])rustc([^[:alnum:]_-]|$)' "$log"; then
        printf 'yes'
      else
        printf 'no'
      fi
    }

    measure_build() {
      local label="$1"
      local target="$2"
      local result="$work/$label.result.json"
      local events="$work/$label.events"
      local log="$work/$label.build.log"
      local planned planned_paths output drv info nar closure paths downloaded rustc wall maxrss
      local impure="''${3:-}"
      local -a nix_options=(--no-write-lock-file)

      if [[ "$impure" == "--impure" ]]; then
        nix_options+=(--impure)
      fi

      planned="$(nix build "''${nix_options[@]}" --dry-run --json "$target")"
      planned_paths="$(jq 'length' <<<"$planned")"
      if ! "${pkgs.time}/bin/time" -f 'wall_seconds=%e\nmaxrss_kb=%M' \
        nix build "''${nix_options[@]}" --no-link --json --log-format internal-json "$target" \
        >"$result" 2>"$events"; then
        cat "$events" >&2
        return 1
      fi
      output="$(jq -r '.[0].outputs.out // (.[0].outputs | to_entries[0].value)' "$result")"
      drv="$(jq -r '.[0].drvPath' "$result")"
      info="$(nix path-info --json --json-format 1 --recursive --closure-size "$output")"
      nar="$(jq -r --arg path "$output" '.[$path].narSize // "unknown"' <<<"$info")"
      closure="$(jq -r --arg path "$output" '.[$path].closureSize // "unknown"' <<<"$info")"
      paths="$(jq 'length' <<<"$info")"
      nix log "$drv" >"$log" 2>/dev/null || :
      downloaded="$(downloaded_metric "$events")"
      rustc="$(rustc_metric "$log")"
      wall="$(read_metric wall_seconds "$events")"
      maxrss="$(read_metric maxrss_kb "$events")"
      last_output="$output"
      printf 'build\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$label" "$wall" "$maxrss" "$downloaded" "$planned_paths" "$nar" "$closure" "$paths" "$rustc" "$output"
    }

    measure_command() {
      local label="$1"
      shift
      local output="$work/$label.stdout"
      local events="$work/$label.events"
      local wall maxrss downloaded rustc
      if ! "${pkgs.time}/bin/time" -f 'wall_seconds=%e\nmaxrss_kb=%M' "$@" \
        >"$output" 2>"$events"; then
        cat "$events" >&2
        return 1
      fi
      downloaded="$(downloaded_metric "$events")"
      rustc="$(rustc_metric "$events")"
      wall="$(read_metric wall_seconds "$events")"
      maxrss="$(read_metric maxrss_kb "$events")"
      printf 'command\t%s\t%s\t%s\t%s\tunknown\tunknown\tunknown\tunknown\t%s\t%s\n' \
        "$label" "$wall" "$maxrss" "$downloaded" "$rustc" "$(tr '\n' ' ' <"$output")"
    }

    echo "# framework products" >&2
    measure_build cli "$framework#nixfied-cli"
    measure_build runtime "$framework#nixfied-runtime"

    if ((source_boundaries)); then
      echo "# isolated source/derivation boundary check" >&2
      measure_build source-boundaries "$framework#checks.$system.package-boundaries" --impure
    fi

    echo "# fresh adopter" >&2
    measure_command install \
      nix run --no-write-lock-file "$framework#install" -- \
        --root "$project" \
        --project-id distribution-benchmark \
        --name "Distribution Benchmark" \
        --nixfied-url "path:$framework"
    # shellcheck disable=SC2016
    measure_command adopter-lock \
      ${pkgs.bash}/bin/bash -c 'cd "$1"; shift; exec nix "$@"' \
      benchmark-shell "$project" flake lock "$project"
    model_drv="$(nix eval --no-write-lock-file --raw "$project#packages.$system.model.drvPath")"
    printf 'model_derivation\t%s\n' "$model_drv"
    measure_command model-eval nix eval --no-write-lock-file --raw "$project#packages.$system.model.drvPath"
    measure_build model "$project#model"
    model_path="$last_output"
    # shellcheck disable=SC2016
    measure_command model-check \
      ${pkgs.bash}/bin/bash -c 'cd "$1"; shift; exec nix "$@"' \
      benchmark-shell "$project" run --no-write-lock-file "$project#model-check"
    # shellcheck disable=SC2016
    measure_command runtime-task \
      ${pkgs.bash}/bin/bash -c 'cd "$1"; shift; exec nix "$@"' \
      benchmark-shell "$project" run --no-write-lock-file "$project#smoke"
    printf 'model_output\t%s\n' "$model_path"
    printf 'note\tclean mode requires an isolated Nix store; warm mode reuses the current store\n'
  '';
}
