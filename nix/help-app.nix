{
  expectedFlakePath,
  pkgs,
  system,
  flakeRef,
}:
let
  target = "${flakeRef}#apps.${system}";
  app = pkgs.writeShellApplication {
    name = "nixfied-help";
    text = ''
      case "$#" in
        0) ;;
        1)
          case "$1" in
            -h|--help) ;;
            *)
              echo "help: unknown argument: $1 (supported: -h, --help)" >&2
              exit 1
              ;;
          esac
          ;;
        *)
          echo "help: expected no arguments, -h, or --help" >&2
          exit 1
          ;;
      esac

      actual_flake_path="$(${pkgs.nix}/bin/nix flake metadata \
        --no-write-lock-file \
        --json ${pkgs.lib.escapeShellArg flakeRef} \
        | ${pkgs.jq}/bin/jq -er '(
            .path + ((.resolved.dir? // "") | if . == "" then "" else "/" + . end)
          ) | select(type == "string" and length > 0)')" \
        || {
          echo "help: could not resolve the current flake source" >&2
          exit 1
        }
      if [ "$actual_flake_path" != ${pkgs.lib.escapeShellArg expectedFlakePath} ]; then
        echo "help: context mismatch; run 'nix run .#help' from the owning flake root" >&2
        exit 1
      fi

      exec ${pkgs.nix}/bin/nix eval \
        --no-write-lock-file \
        --raw ${pkgs.lib.escapeShellArg target} \
        --apply "$(<${./help-renderer.nix})"
    '';
  };
in
{
  type = "app";
  program = "${app}/bin/nixfied-help";
  meta.description = "List this flake's runnable commands";
}
