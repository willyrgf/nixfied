#!/usr/bin/env bash

require_executable_env_var() {
  local var_name="$1"
  local value="${!var_name:-}"

  if [ -z "$value" ] || [ ! -x "$value" ]; then
    echo "${var_name} not executable: ${value:-<unset>}" >&2
    return 1
  fi
}
