# Playwright module (optional)
{ pkgs, project, isLinux ? pkgs.stdenv.isLinux }:

let
  cfg = project.modules.playwright or { };
  browsersPath = cfg.browsersPath or "";

  envSetup = pkgs.writeShellScript "playwright-env-setup" ''
    if [ -n "${browsersPath}" ]; then
      export PLAYWRIGHT_BROWSERS_PATH="${browsersPath}"
    fi
  '';
in
{
  inherit envSetup;
}
