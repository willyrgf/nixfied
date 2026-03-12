{
  mkCommandTask,
  frameworkInstallRuntimeInputs,
  frameworkInstallContractArgs,
  frameworkUpgradeContractArgs,
  mkFrameworkInstallCommand,
  ownerFile ? "nixfied/project/module.nix",
}:
{
  tasks = {
    framework-install = mkCommandTask {
      id = "task.framework.install";
      appName = "framework::install";
      kind = "utility";
      summary = "Install thin or vendored wrapper flake";
      description = "Creates a thin wrapper flake by default, or a vendored wrapper with --vendor. Re-running with --vendor preserves nixfied/project and nixfied/local by default.";
      runtimeInputs = frameworkInstallRuntimeInputs;
      usage = [
        "nix run .#framework::install"
        "nix run .#framework::install -- --vendor"
        "nix run .#framework::install -- --vendor --target ."
        "nix run .#framework::install -- --vendor --upgrade --target ."
      ];
      contractArgs = frameworkInstallContractArgs;
      command = mkFrameworkInstallCommand { };
      inherit ownerFile;
    };

    framework-upgrade = mkCommandTask {
      id = "task.framework.upgrade";
      appName = "framework::upgrade";
      kind = "utility";
      summary = "Upgrade vendored wrapper in-place";
      description = "Upgrades framework files while preserving nixfied/project and nixfied/local by default. Use --reset-project/--reset-local to overwrite those paths.";
      runtimeInputs = frameworkInstallRuntimeInputs;
      usage = [
        "nix run .#framework::upgrade -- --target ."
        "nix run .#framework::upgrade -- --target . --reset-project"
        "nix run .#framework::upgrade -- --target . --reset-local"
      ];
      contractArgs = frameworkUpgradeContractArgs;
      command = mkFrameworkInstallCommand {
        upgradeDefault = true;
      };
      inherit ownerFile;
    };
  };
}
