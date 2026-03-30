# Nix builders - mkAppScript, mkApp, mkAppWithDeps, withTiming
{
  pkgs,
  project,
  shellContract,
  fixtureLib,
  loadEnv,
  helpersScript,
  hookExports,
}:

let
  commandWrapper = import ./command-wrapper.nix {
    inherit
      pkgs
      project
      shellContract
      loadEnv
      helpersScript
      hookExports
      ;
  };

  # Timing wrapper - records and displays execution time
  withTiming = name: script: ''
    _TIMING_START=$(date +%s)
    _TIMING_EXIT=0

    (
      set -euo pipefail
      ${script}
    ) || _TIMING_EXIT=$?

    _TIMING_END=$(date +%s)
    _TIMING_DURATION=$((_TIMING_END - _TIMING_START))

    echo ""
    if [ $_TIMING_DURATION -lt 60 ]; then
      log_timing "${name} duration=''${_TIMING_DURATION}s"
    else
      _TIMING_MINS=$((_TIMING_DURATION / 60))
      _TIMING_SECS=$((_TIMING_DURATION % 60))
      log_timing "${name} duration=''${_TIMING_MINS}m''${_TIMING_SECS}s"
    fi

    exit $_TIMING_EXIT
  '';

  # Helper to create app scripts
  mkAppScript =
    {
      name,
      script,
      fixtures ? null,
      env ? { },
      useDeps ? false,
      fixtureProfile ? "default",
      commandApi ? null,
    }:
    let
      depsScript = project.install.deps or "";
      depsBlock = if useDeps && depsScript != "" then depsScript else "";
      script0 = fixtureLib.wrapScript {
        contextName = name;
        inherit fixtures;
        defaultProfile = fixtureProfile;
        defaultLogs = true;
        script = script;
      };
    in
    commandWrapper.mkCommandWrappedScript {
      inherit
        name
        env
        commandApi
        ;
      beforeContract = depsBlock;
      script = script0;
    };

  mkApp =
    {
      name,
      script,
      fixtures ? null,
      env ? { },
      useDeps ? false,
      fixtureProfile ? "default",
      description ? null,
      meta ? { },
      api ? null,
    }:
    let
      scriptDrv = mkAppScript {
        inherit
          name
          script
          fixtures
          env
          useDeps
          fixtureProfile
          ;
        commandApi = if api == null then null else (api.commandApi or null);
      };
    in
    {
      type = "app";
      meta = pkgs.lib.recursiveUpdate (pkgs.lib.optionalAttrs (description != null) {
        inherit description;
      }) meta;
      program = toString scriptDrv;
    };

  mkAppWithDeps = args: mkApp (args // { useDeps = true; });
in
{
  inherit
    withTiming
    mkAppScript
    mkApp
    mkAppWithDeps
    ;
}
