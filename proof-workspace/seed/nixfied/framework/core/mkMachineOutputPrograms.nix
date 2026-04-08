{
  pkgs,
  appId,
  app,
  contractBundle,
  targetProgram,
  setupPrograms ? [ ],
  teardownPrograms ? [ ],
}:
let
  inherit (pkgs) lib;
  mkShellApp = import ./mk-shell-app.nix { inherit pkgs; };
  kernelPackage = import ../runtime/kernel { inherit pkgs; };
  contractRef = (app.validation or { }).contractRef or "";
  bundleFile = pkgs.writeText "nixfied-machine-output-contract-bundle.json" (
    builtins.toJSON contractBundle.bundle
  );
  runtimePlan = pkgs.writeText "nixfied-machine-output-${appId}.json" (
    builtins.toJSON {
      kind = "nixfied-machine-output-plan";
      version = 1;
      inherit appId;
      targetAppId = app.targetAppId or "";
      inherit contractRef;
      inherit bundleFile;
      inherit targetProgram;
      inherit setupPrograms;
      inherit teardownPrograms;
      targetArgs = app.targetArgs or [ ];
    }
  );
in
(mkShellApp {
  appName = "machine-output:${appId}";
  binPrefix = "nixfied-machine-output";
  body = ''
    exec ${kernelPackage}/bin/nixfied-kernel machine-output run ${lib.escapeShellArg runtimePlan} -- "$@"
  '';
}).program
