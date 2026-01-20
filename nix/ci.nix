# CI pipeline generator (steps DSL)
{
  pkgs,
  project,
  lib,
}:

let
  ci = project.ci or null;
  enabled = ci != null && (ci.enable or false);

  steps = if enabled then (ci.steps or { }) else { };
  modes = if enabled then (ci.modes or { }) else { };
  modeNames = builtins.attrNames modes;
  defaultMode =
    if enabled then
      (ci.defaultMode or (if modeNames != [ ] then builtins.head modeNames else ""))
    else
      "";
  resolvedDefaultMode =
    if defaultMode != "" && builtins.hasAttr defaultMode modes then
      defaultMode
    else if modeNames != [ ] then
      builtins.head modeNames
    else
      "";

  ciEnv = ci.env or { };
  ciEnvExports = pkgs.lib.concatMapStringsSep "\n" (
    key: "export ${key}=${toString ciEnv.${key}}"
  ) (builtins.attrNames ciEnv);

  artifacts = ci.artifacts or { };
  artifactsDir = artifacts.dir or "/tmp/ci-artifacts";
  keepOnFailure = artifacts.keepOnFailure or true;
  keepOnSuccess = artifacts.keepOnSuccess or false;

  stepDescCase =
    pkgs.lib.concatMapStringsSep "\n" (
      name:
      let
        desc = steps.${name}.description or name;
      in
      "  ${name}) echo \"${desc}\" ;;"
    ) (builtins.attrNames steps);

  normalizeName = name:
    let
      replaced = pkgs.lib.replaceStrings [ "-" "." " " "/" ] [ "_" "_" "_" "_" ] name;
    in
    pkgs.lib.strings.toLower replaced;

  stepFuncCase =
    pkgs.lib.concatMapStringsSep "\n" (
      name:
      let
        slug = normalizeName name;
      in
      "  ${name}) echo \"run_step_${slug}\" ;;"
    ) (builtins.attrNames steps);

  modeCase =
    pkgs.lib.concatMapStringsSep "\n" (
      mode:
      let
        modeSteps = modes.${mode}.steps or [ ];
        stepsList = pkgs.lib.concatMapStringsSep " " (s: "\"${s}\"") modeSteps;
      in
      "  ${mode}) STEPS=(${stepsList}) ;;"
    ) modeNames;

  mkStepFunction =
    name: step:
    let
      desc = step.description or name;
      run = step.run or "";
      when = step.when or "";
      cleanup = step.cleanup or "";
      env = step.env or { };
      slug = normalizeName name;
      envExports = pkgs.lib.concatMapStringsSep "\n" (
        key: "export ${key}=${toString env.${key}}"
      ) (builtins.attrNames env);
      skipVars = step.skipIfMissing or [ ];
      skipList = pkgs.lib.concatMapStringsSep " " (v: "\"${v}\"") skipVars;
      requires = step.requires or [ ];
      missingModules =
        builtins.filter (req:
          let
            modCfg = project.modules.${req} or null;
            enabledMod = if modCfg == null then false else (modCfg.enable or false);
          in
          !enabledMod
        ) requires;
      missingReason =
        if missingModules == [ ] then
          ""
        else
          "requires module(s): ${pkgs.lib.concatStringsSep ", " missingModules}";
    in
    ''
      run_step_${slug}() {
        local step_name="${name}"
        local step_desc="${desc}"

${pkgs.lib.optionalString (missingReason != "") ''
        echo "↷ Skipping ''${step_desc}: ${missingReason}"
        return 0
''}

${pkgs.lib.optionalString (skipVars != [ ]) ''
        local missing_reason=""
        for var in ${skipList}; do
          if [ -z "''${!var:-}" ]; then
            missing_reason="missing $var"
            break
          fi
        done
        if [ -n "$missing_reason" ]; then
          echo "↷ Skipping ''${step_desc}: $missing_reason"
          return 0
        fi
''}

${pkgs.lib.optionalString (when != "") ''
        if ! ( ${when} ); then
          echo "↷ Skipping ''${step_desc}: condition not met"
          return 0
        fi
''}

        local rc=0
        set +e
        (
          set -euo pipefail
${pkgs.lib.optionalString (envExports != "") envExports}
${run}
        )
        rc=$?
        set -e
${pkgs.lib.optionalString (cleanup != "") cleanup}
        if [ $rc -ne 0 ]; then
          return $rc
        fi
        return 0
      }
    '';

  stepFunctions =
    pkgs.lib.concatMapStringsSep "\n" (name: mkStepFunction name steps.${name})
      (builtins.attrNames steps);

  setupScript = ci.setup or "";
  teardownScript = ci.teardown or "";

  script =
    if !enabled then
      ''
        echo "CI DSL is disabled. Enable it in nix/project/ci.nix (ci.enable = true)."
        exit 1
      ''
    else if modeNames == [ ] then
      ''
        echo "No CI modes configured (ci.modes is empty)."
        exit 1
      ''
    else
      ''
        # Parse args
        CI_MODE="${resolvedDefaultMode}"
        CI_SUMMARY=false
        CI_STEP_ARGS=()

        while [ "''$#" -gt 0 ]; do
          case "''$1" in
            --summary)
              CI_SUMMARY=true
              shift
              ;;
            --mode)
              CI_MODE="''${2:-}"
              shift 2
              ;;
            --)
              shift
              CI_STEP_ARGS=("$@")
              break
              ;;
            --*)
              MODE_FLAG="''${1#--}"
              case "$MODE_FLAG" in
${pkgs.lib.concatMapStringsSep "\n" (m: "                ${m}) CI_MODE=\"${m}\" ;;") modeNames}
                *)
                  echo "Unknown option: ''$1" >&2
                  exit 1
                  ;;
              esac
              shift
              ;;
            *)
              echo "Unknown option: ''$1" >&2
              exit 1
              ;;
          esac
        done

        export CI_MODE
        export CI_SUMMARY
        export CI_STEP_ARGS
        export CI_ARTIFACTS_DIR="${artifactsDir}"
        export CI_KEEP_ARTIFACTS_ON_FAILURE="${if keepOnFailure then "1" else "0"}"
        export CI_KEEP_ARTIFACTS_ON_SUCCESS="${if keepOnSuccess then "1" else "0"}"

${stepFunctions}

        step_desc() {
          case "''$1" in
${stepDescCase}
            *) echo "''$1" ;;
          esac
        }

        step_func() {
          case "''$1" in
${stepFuncCase}
            *) echo "" ;;
          esac
        }

        run_pipeline() {
          set -euo pipefail
${pkgs.lib.optionalString (ciEnvExports != "") ciEnvExports}
${setupScript}

          mkdir -p "$CI_ARTIFACTS_DIR"

          case "$CI_MODE" in
${modeCase}
            *)
              echo "Unknown CI mode: $CI_MODE" >&2
              exit 1
              ;;
          esac

          if [ "''${#STEPS[@]}" -eq 0 ]; then
            echo "No steps configured for mode: $CI_MODE"
            exit 1
          fi

          TOTAL_STEPS="''${#STEPS[@]}"
          STEP_INDEX=1
          for step in "''${STEPS[@]}"; do
            STEP_FUNC=$(step_func "$step")
            if [ -z "$STEP_FUNC" ]; then
              echo "Unknown step: $step" >&2
              exit 1
            fi
            STEP_DESC=$(step_desc "$step")
            echo ""
            echo "Step ''${STEP_INDEX}/''${TOTAL_STEPS}: ''${STEP_DESC}"
            "$STEP_FUNC"
            STEP_INDEX=$((STEP_INDEX + 1))
          done

${teardownScript}
        }

        if [ "$CI_SUMMARY" = "true" ]; then
          LOGFILE=$(mktemp)
          trap "rm -f $LOGFILE" EXIT
          START_TIME=$(date +%s)
          set +e
          ( run_pipeline ) 2>&1 | tee "$LOGFILE"
          EXIT_CODE=$?
          set -e
          END_TIME=$(date +%s)
          DURATION=$((END_TIME - START_TIME))
          summary_parse "$LOGFILE" "$DURATION" "$EXIT_CODE"
          if [ "$EXIT_CODE" -ne 0 ]; then
            if [ "$CI_KEEP_ARTIFACTS_ON_FAILURE" = "1" ]; then
              echo "🧾 CI artifacts kept at: $CI_ARTIFACTS_DIR"
            else
              rm -rf "$CI_ARTIFACTS_DIR" 2>/dev/null || true
            fi
          else
            if [ "$CI_KEEP_ARTIFACTS_ON_SUCCESS" = "1" ]; then
              echo "🧾 CI artifacts kept at: $CI_ARTIFACTS_DIR"
            else
              rm -rf "$CI_ARTIFACTS_DIR" 2>/dev/null || true
            fi
          fi
          exit $EXIT_CODE
        else
          set +e
          run_pipeline
          EXIT_CODE=$?
          set -e
          if [ "$EXIT_CODE" -ne 0 ]; then
            if [ "$CI_KEEP_ARTIFACTS_ON_FAILURE" = "1" ]; then
              echo "🧾 CI artifacts kept at: $CI_ARTIFACTS_DIR"
            else
              rm -rf "$CI_ARTIFACTS_DIR" 2>/dev/null || true
            fi
          else
            if [ "$CI_KEEP_ARTIFACTS_ON_SUCCESS" = "1" ]; then
              echo "🧾 CI artifacts kept at: $CI_ARTIFACTS_DIR"
            else
              rm -rf "$CI_ARTIFACTS_DIR" 2>/dev/null || true
            fi
          fi
          exit $EXIT_CODE
        fi
      '';
in
if enabled then
  lib.mkApp {
    name = "ci";
    description = "Run the CI pipeline";
    env = ci.env or { };
    useDeps = ci.useDeps or true;
    script = script;
  }
else
  null
