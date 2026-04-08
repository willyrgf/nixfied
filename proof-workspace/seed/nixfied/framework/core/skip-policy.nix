# Shared shell helpers for runtime-owned service exclusion parsing.
{ pkgs }:

{
  skipPolicyFunctions = ''
    normalize_excluded_service_name() {
      local service_name="$1"
      printf '%s' "$service_name" | ${pkgs.coreutils}/bin/tr -d '[:space:]'
    }

    excluded_services_contains() {
      local service_name="$1"
      local normalized_service=""
      local excluded_csv="''${NIXFIED_EXCLUDED_SERVICES_CSV:-}"
      local old_ifs="$IFS"
      local excluded_parts=()
      local candidate=""

      normalized_service="$(normalize_excluded_service_name "$service_name")"
      if [ -z "$normalized_service" ] || [ -z "$excluded_csv" ]; then
        return 1
      fi

      IFS=','
      read -r -a excluded_parts <<< "$excluded_csv"
      IFS="$old_ifs"

      for candidate in "''${excluded_parts[@]}"; do
        candidate="$(normalize_excluded_service_name "$candidate")"
        if [ -n "$candidate" ] && [ "$candidate" = "$normalized_service" ]; then
          return 0
        fi
      done

      return 1
    }

    filter_excluded_services_csv() {
      local input_csv="$1"
      local old_ifs="$IFS"
      local parts=()
      local filtered=()
      local service_name=""

      if [ -z "$input_csv" ]; then
        printf '%s' ""
        return 0
      fi

      IFS=','
      read -r -a parts <<< "$input_csv"
      IFS="$old_ifs"

      for service_name in "''${parts[@]}"; do
        service_name="$(normalize_excluded_service_name "$service_name")"
        if [ -z "$service_name" ]; then
          continue
        fi
        if excluded_services_contains "$service_name"; then
          continue
        fi
        filtered+=("$service_name")
      done

      IFS=','
      printf '%s' "''${filtered[*]}"
      IFS="$old_ifs"
    }

    is_service_skipped() {
      excluded_services_contains "$1"
    }
  '';
}
