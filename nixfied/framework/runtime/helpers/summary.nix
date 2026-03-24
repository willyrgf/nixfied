# Summary parser - render the validated summary envelope through the kernel.
{
  pkgs,
  project ? { },
  loggingPrelude ? "",
}:

let
  kernelPackage = import ../kernel { inherit pkgs; };

  summaryParser = pkgs.writeShellScript "summary-parser" ''
    ${loggingPrelude}

    LOGFILE="$1"
    DURATION="$2"
    EXIT_CODE="$3"

    SUMMARY_JSON=""
    if [ -n "''${CI_ARTIFACTS_DIR:-}" ] && [ -f "$CI_ARTIFACTS_DIR/summary.json" ]; then
      SUMMARY_JSON="$CI_ARTIFACTS_DIR/summary.json"
    elif [ -n "$LOGFILE" ]; then
      PARENT_DIR=$(dirname "$LOGFILE" 2>/dev/null || true)
      if [ -n "$PARENT_DIR" ] && [ -f "$PARENT_DIR/summary.json" ]; then
        SUMMARY_JSON="$PARENT_DIR/summary.json"
      fi
    fi

    if [ -n "$SUMMARY_JSON" ] && [ -f "$SUMMARY_JSON" ]; then
      ${kernelPackage}/bin/nixfied-kernel summary render-human "$SUMMARY_JSON"
    else
      echo ""
      echo "------------------------------------------------------------"
      echo "Summary"
      echo "------------------------------------------------------------"
      if [ "$EXIT_CODE" -eq 0 ] 2>/dev/null; then
        log_ok "Exit code: 0"
      else
        log_error "Exit code: $EXIT_CODE" 2>&1
      fi
      if [ -n "''${CI_ARTIFACTS_DIR:-}" ] && [ -d "''${CI_ARTIFACTS_DIR}" ]; then
        echo ""
        echo "Artifacts: $CI_ARTIFACTS_DIR"
      fi
      echo "------------------------------------------------------------"
    fi
  '';
in
{
  inherit summaryParser;
}
