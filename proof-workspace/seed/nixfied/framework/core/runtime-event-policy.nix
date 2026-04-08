{
  pkgs,
  project ? { },
}:

let
  projectMeta = project.project or { };
  projectId = projectMeta.id or "project";
  projectIdUpper =
    let
      replaced = pkgs.lib.replaceStrings [ "-" "." ] [ "_" "_" ] projectId;
    in
    pkgs.lib.strings.toUpper replaced;
  slotVar = projectMeta.slotVar or "NIX_ENV";
  envVar = projectMeta.envVar or "PROJECT_ENV";
  processCfg = project.process or { };
  registryRoot =
    if project ? state && project.state ? policy && project.state.policy ? registryRoot then
      project.state.policy.registryRoot
    else if project ? state && project.state ? registry && project.state.registry ? root then
      project.state.registry.root
    else if project ? state && project.state ? registryRoot then
      project.state.registryRoot
    else
      processCfg.registryRoot
        or "\${NIX_BUILD_TOP:-\${XDG_CACHE_HOME:-$HOME/.cache}}/nixfied-runtime/${projectId}/registry";
  baseDirExpr =
    if project ? state && project.state ? policy && project.state.policy ? runtimeBase then
      project.state.policy.runtimeBase
    else
      (project.directories.base or "\${XDG_DATA_HOME:-$HOME/.local/share}/${projectId}");
  ciCfg = project.ci or { };
  artifactsCfg = ciCfg.artifacts or { };
  artifactsRootExpr =
    if project ? state && project.state ? policy && project.state.policy ? artifactsRoot then
      project.state.policy.artifactsRoot
    else
      artifactsCfg.dir or "/tmp/ci-artifacts";
  ephemeralPrefix = "/tmp/${projectId}-ephemeral-";
in
{
  inherit
    projectId
    projectIdUpper
    slotVar
    envVar
    registryRoot
    baseDirExpr
    artifactsRootExpr
    ephemeralPrefix
    ;
  ephemeralFlagVar = "${projectIdUpper}_EPHEMERAL";
  ephemeralRootVar = "${projectIdUpper}_EPHEMERAL_ROOT";

  indexShellFunctions = ''
    runtime_index_segment() {
      local value="$1"
      if [ -z "$value" ]; then
        printf '%s' "__empty__"
        return 0
      fi
      value="''${value//\//_}"
      value="''${value//$'\n'/_}"
      value="''${value//$'\r'/_}"
      value="''${value//$'\t'/_}"
      printf '%s' "$value"
    }

    runtime_events_index_root() {
      printf '%s/runtime-events' "$REGISTRY_ROOT"
    }

    service_events_root_for() {
      local service_name="$1"
      printf '%s/services/%s' "$(runtime_events_index_root)" "$(runtime_index_segment "$service_name")"
    }

    service_events_index_file_for() {
      local service_name="$1"
      local slot_name="$2"
      local env_name="$3"
      printf '%s/%s/%s/events.tsv' \
        "$(service_events_root_for "$service_name")" \
        "$(runtime_index_segment "$slot_name")" \
        "$(runtime_index_segment "$env_name")"
    }

    slot_events_index_file_for() {
      local slot_name="$1"
      local env_name="$2"
      printf '%s/slots/%s/%s/events.tsv' \
        "$(runtime_events_index_root)" \
        "$(runtime_index_segment "$slot_name")" \
        "$(runtime_index_segment "$env_name")"
    }
  '';
}
