{
  lib,
  pkgs,
  project,
  slots,
}:
let
  sanitizeShellName =
    value: lib.replaceStrings [ "-" "." ":" "/" " " ] [ "_" "_" "_" "_" "_" ] value;

  sanitizeFileName =
    value: lib.replaceStrings [ "-" "." ":" "/" " " ] [ "-" "-" "-" "-" "-" ] value;

  getServiceEntry =
    serviceName:
    let
      services = project.services or { };
      serviceId = "service.${serviceName}";
    in
    if builtins.hasAttr serviceId services then
      services.${serviceId}
    else if builtins.hasAttr serviceName services then
      services.${serviceName}
    else
      { };

  mkPrelude =
    {
      serviceName,
      displayName,
      dataDirName ? serviceName,
      logFileName ? "${serviceName}.log",
      pidFileName ? "${serviceName}.pid",
    }:
    let
      serviceEntry = getServiceEntry serviceName;
      cfg = serviceEntry.config or { };
      endpoints = cfg.resolved.endpoints or { };
      endpointNames = builtins.sort builtins.lessThan (builtins.attrNames endpoints);
      selectedSource = cfg.resolved.selectedSource or cfg.defaultSource or "";
      sourceKinds = cfg.sourceKinds or { };
      sourceKind =
        if selectedSource != "" && builtins.hasAttr selectedSource sourceKinds then
          sourceKinds.${selectedSource}
        else
          "unknown";
      portAssignments = builtins.concatStringsSep "\n" (
        map (
          endpointName:
          let
            shellName = sanitizeShellName endpointName;
            envVar = slots.portVarName endpoints.${endpointName}.portKey;
          in
          ''port_${shellName}="''${${envVar}:-}"''
        ) endpointNames
      );
      metadataLines = builtins.concatStringsSep "\n" (
        map (
          endpointName:
          let
            shellName = sanitizeShellName endpointName;
          in
          ''printf 'port.${endpointName}=%s\n' "''${port_${shellName}}"''
        ) endpointNames
      );
      defaultDatabase = cfg.database or "app";
      testDatabase = cfg.testDatabase or "app_test";
      network = cfg.network or "local";
      rootUser = cfg.rootUser or "minioadmin";
      rootPassword = cfg.rootPassword or "minioadmin";
    in
    ''
      set -euo pipefail
      source <(${slots.getSlotInfo})

      service_name=${lib.escapeShellArg serviceName}
      display_name=${lib.escapeShellArg displayName}
      data_dir_name=${lib.escapeShellArg dataDirName}
      selected_source=${lib.escapeShellArg selectedSource}
      source_kind=${lib.escapeShellArg sourceKind}
      default_database=${lib.escapeShellArg defaultDatabase}
      test_database=${lib.escapeShellArg testDatabase}
      network_name=${lib.escapeShellArg network}
      minio_root_user=${lib.escapeShellArg rootUser}
      minio_root_password=${lib.escapeShellArg rootPassword}

      service_dir="$NIXFIED_SERVICE_ROOT/$data_dir_name"
      run_dir="$service_dir/run"
      log_dir="$service_dir/logs"
      config_dir="$service_dir/config"
      data_dir="$service_dir/data"
      state_dir="$service_dir/state"
      site_dir="$config_dir/sites"
      enabled_site_dir="$config_dir/sites-enabled"
      policy_dir="$config_dir/policies"
      cert_dir="$config_dir/certs"
      backup_dir="$state_dir/backups"
      migration_dir="$state_dir/migrations"
      bucket_dir="$data_dir/buckets"
      db_dir="$data_dir/databases"
      pid_file="$run_dir/${pidFileName}"
      log_file="$log_dir/${logFileName}"
      status_file="$state_dir/status"
      running_marker="$state_dir/running"
      operations_log="$state_dir/operations.log"
      metadata_file="$state_dir/metadata.env"
      workflow_phase_log_file="''${PROOF_WORKFLOW_PHASE_LOG_FILE:-}"

      mkdir -p \
        "$run_dir" \
        "$log_dir" \
        "$config_dir" \
        "$data_dir" \
        "$state_dir" \
        "$site_dir" \
        "$enabled_site_dir" \
        "$policy_dir" \
        "$cert_dir" \
        "$backup_dir" \
        "$migration_dir" \
        "$bucket_dir" \
        "$db_dir"

      ${portAssignments}

      append_operation() {
        local op="$1"
        printf '%s\n' "$op" >> "$operations_log"
        printf '%s\n' "$op" >> "$log_file"
        if [ -n "$workflow_phase_log_file" ]; then
          case "$op" in
            ready:*|health:*)
              printf '%s\n' "$op" >> "$workflow_phase_log_file"
              ;;
          esac
        fi
      }

      write_metadata() {
        {
          printf 'service=%s\n' "$service_name"
          printf 'display_name=%s\n' "$display_name"
          printf 'source=%s\n' "$selected_source"
          printf 'source_kind=%s\n' "$source_kind"
          printf 'slot=%s\n' "''${SLOT:-}"
          printf 'env=%s\n' "''${ENV:-}"
          printf 'network=%s\n' "$network_name"
          printf 'service_dir=%s\n' "$service_dir"
          printf 'data_dir=%s\n' "$data_dir"
          printf 'default_database=%s\n' "$default_database"
          printf 'test_database=%s\n' "$test_database"
          ${metadataLines}
        } > "$metadata_file"
      }

      is_running() {
        [ -f "$running_marker" ]
      }

      require_running() {
        if ! is_running; then
          append_operation "not-running:$service_name"
          echo "ERROR: service=$service_name state=stopped"
          exit 1
        fi
      }

      mark_running() {
        : > "$running_marker"
        printf '%s\n' "$$" > "$pid_file"
        printf '%s\n' "running" > "$status_file"
      }

      mark_stopped() {
        rm -f "$running_marker" "$pid_file"
        printf '%s\n' "stopped" > "$status_file"
      }

      write_metadata
      touch "$log_file"
    '';
in
{
  mkBaseOperations =
    args@{
      serviceName,
      ...
    }:
    let
      prelude = mkPrelude args;
      mkCommand =
        opName: body:
        pkgs.writeShellScript "${sanitizeFileName serviceName}-${sanitizeFileName opName}" ''
          ${prelude}
          ${body}
        '';
    in
    {
      inherit mkCommand;
      operations = {
        init = mkCommand "init" ''
          append_operation "init:$service_name"
          echo "OK: service=$service_name initialized"
        '';

        "pre-start" = mkCommand "pre-start" ''
          append_operation "pre-start:$service_name"
          echo "INFO: service=$service_name pre-start"
        '';

        "preflight-start" = mkCommand "preflight-start" ''
          if [ -z "$selected_source" ]; then
            echo "ERROR: service=$service_name source=unset"
            exit 1
          fi
          append_operation "preflight-start:$service_name"
          echo "OK: service=$service_name preflight=passed source=$selected_source source_kind=$source_kind"
        '';

        "check-config" = mkCommand "check-config" ''
          append_operation "check-config:$service_name"
          echo "OK: service=$service_name config=valid"
        '';

        "start-leaf" = mkCommand "start-leaf" ''
          mark_running
          append_operation "start:$service_name"
          echo "OK: service=$service_name state=running source=$selected_source"
        '';

        "pre-stop" = mkCommand "pre-stop" ''
          append_operation "pre-stop:$service_name"
          echo "INFO: service=$service_name pre-stop"
        '';

        stop = mkCommand "stop" ''
          mark_stopped
          append_operation "stop:$service_name"
          echo "OK: service=$service_name state=stopped"
        '';

        status = mkCommand "status" ''
          state="stopped"
          if is_running; then
            state="running"
          fi
          append_operation "status:$service_name"
          echo "INFO: service=$service_name state=$state source=$selected_source"
        '';

        health = mkCommand "health" ''
          if ! is_running; then
            append_operation "health:$service_name"
            echo "ERROR: service=$service_name health=down"
            exit 1
          fi
          append_operation "health:$service_name"
          echo "OK: service=$service_name health=up"
        '';

        ready = mkCommand "ready" ''
          if ! is_running; then
            append_operation "ready:$service_name"
            echo "ERROR: service=$service_name ready=down"
            exit 1
          fi
          append_operation "ready:$service_name"
          echo "OK: service=$service_name ready=up"
        '';

        "full-start-leaf" = mkCommand "full-start-leaf" ''
          mark_running
          append_operation "full-start:$service_name"
          echo "OK: service=$service_name full-start=complete"
        '';

        "full-start-test-leaf" = mkCommand "full-start-test-leaf" ''
          mark_running
          append_operation "full-start-test:$service_name"
          echo "OK: service=$service_name full-start-test=complete"
        '';
      };
    };
}
