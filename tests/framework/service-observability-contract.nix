{ pkgs }:
let
  helperSource = builtins.readFile ../../nixfied/.framework/lib/service-observability.nix;
  minioSource = builtins.readFile ../../nixfied/.framework/minio/lifecycle.nix;
  nginxSource = builtins.readFile ../../nixfied/.framework/nginx/lifecycle.nix;
  rethSource = builtins.readFile ../../nixfied/.framework/reth/lifecycle.nix;
  heliosSource = builtins.readFile ../../nixfied/.framework/helios/lifecycle.nix;
  postgresSource = builtins.readFile ../../nixfied/.framework/postgres/lifecycle.nix;
in
assert pkgs.lib.hasInfix "mkStatusLine =" helperSource;
assert pkgs.lib.hasInfix "beforeRunningFields ? [ ]" helperSource;
assert pkgs.lib.hasInfix "afterPidFields ? [ ]" helperSource;
assert pkgs.lib.hasInfix "owner_run_id=" helperSource;
assert pkgs.lib.hasInfix "registry_state=" helperSource;
assert pkgs.lib.hasInfix "slot_owner=" helperSource;
assert pkgs.lib.hasInfix "wait_reason=" helperSource;
assert pkgs.lib.hasInfix "log_path=$EFFECTIVE_LOG_PATH" helperSource;
assert pkgs.lib.hasInfix "statusBody = observability.mkStatusLine {" minioSource;
assert pkgs.lib.hasInfix "statusBody = observability.mkStatusLine {" nginxSource;
assert pkgs.lib.hasInfix "statusBody = observability.mkStatusLine {" rethSource;
assert pkgs.lib.hasInfix "statusBody = observability.mkStatusLine {" heliosSource;
assert pkgs.lib.hasInfix "statusBody = observability.mkStatusLine {" postgresSource;
assert (!pkgs.lib.hasInfix "echo \"service=minio" minioSource);
assert (!pkgs.lib.hasInfix "echo \"service=nginx" nginxSource);
assert (!pkgs.lib.hasInfix "echo \"service=reth" rethSource);
assert (!pkgs.lib.hasInfix "echo \"service=helios" heliosSource);
assert (!pkgs.lib.hasInfix "echo \"service=postgres" postgresSource);
pkgs.runCommand "service-observability-contract" { } ''
  echo "OK: service observability owns shared service status formatting" > "$out"
''
