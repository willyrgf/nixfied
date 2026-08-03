# Prove that package selection is a source/build boundary, not only a Cargo
# flag. The temporary source variants are deliberately test-only inputs: they
# make isolated source changes observable without adding a public package path.
{
  pkgs,
  publicPackages,
  runSourceMatrix ? true,
}:
let
  source = ../../runtime;
  lockFiles = {
    nixfied-cli = ../../runtime/locks/nixfied-cli.Cargo.lock;
    nixfied-runtime = ../../runtime/Cargo.lock;
    nixfied-test-child = ../../runtime/locks/nixfied-test-child.Cargo.lock;
  };
  rustToolchain = (import ../toolchain.nix { inherit pkgs; }).build;
  mkSourceVariant =
    name: relativePath:
    pkgs.runCommand "nixfied-package-boundary-${name}" { } ''
      mkdir -p "$out"
      cp -R --no-preserve=mode,ownership ${source}/. "$out/"
      printf '\n// package-boundary source variant\n' >> "$out/${relativePath}"
    '';
  mkPackage =
    {
      package,
      buildType ? "release",
      sourceOverride ? source,
    }:
    import ../packages/runtime.nix {
      inherit pkgs package buildType;
      source = sourceOverride;
    };
  cliSource = import ../packages/runtime-source.nix {
    inherit pkgs source;
    package = "nixfied-cli";
    lockFile = lockFiles.nixfied-cli;
  };
  runtimeSource = import ../packages/runtime-source.nix {
    inherit pkgs source;
    package = "nixfied-runtime";
    lockFile = lockFiles.nixfied-runtime;
  };
  testChildSource = import ../packages/runtime-source.nix {
    inherit pkgs source;
    package = "nixfied-test-child";
    lockFile = lockFiles.nixfied-test-child;
  };
  cliVariant = mkSourceVariant "cli" "crates/nixfied-cli/src/main.rs";
  runtimeVariant = mkSourceVariant "runtime" "crates/nixfied-runtime/src/lib.rs";
  modelVariant = mkSourceVariant "model" "crates/nixfied-model/src/lib.rs";
  testChildVariant = mkSourceVariant "test-child" "crates/nixfied-test-child/src/main.rs";
  cli = mkPackage { package = "nixfied-cli"; };
  runtime = mkPackage { package = "nixfied-runtime"; };
  debugRuntime = mkPackage {
    package = "nixfied-runtime";
    buildType = "debug";
  };
  cliFromCliChange = mkPackage {
    package = "nixfied-cli";
    sourceOverride = cliVariant;
  };
  runtimeFromCliChange = mkPackage {
    package = "nixfied-runtime";
    sourceOverride = cliVariant;
  };
  cliFromRuntimeChange = mkPackage {
    package = "nixfied-cli";
    sourceOverride = runtimeVariant;
  };
  runtimeFromRuntimeChange = mkPackage {
    package = "nixfied-runtime";
    sourceOverride = runtimeVariant;
  };
  cliFromModelChange = mkPackage {
    package = "nixfied-cli";
    sourceOverride = modelVariant;
  };
  runtimeFromModelChange = mkPackage {
    package = "nixfied-runtime";
    sourceOverride = modelVariant;
  };
  cliFromTestChildChange = mkPackage {
    package = "nixfied-cli";
    sourceOverride = testChildVariant;
  };
  runtimeFromTestChildChange = mkPackage {
    package = "nixfied-runtime";
    sourceOverride = testChildVariant;
  };
  sourceStaging = pkgs.runCommand "nixfied-package-boundary-sources" { } ''
    mkdir -p "$out"
    cp ${source}/Cargo.lock "$out/Cargo.lock"
    cp -R ${cliSource.root} "$out/cli"
    cp -R ${runtimeSource.root} "$out/runtime"
    cp -R ${testChildSource.root} "$out/test-child"
  '';
  drvPath = derivation: builtins.unsafeDiscardStringContext derivation.drvPath;
  sourceBoundaryAssertions =
    if runSourceMatrix then
      ''
        test "${drvPath cli}" != "${drvPath cliFromCliChange}" \
          || fail "CLI-only source change did not change the CLI derivation"
        test "${drvPath runtime}" = "${drvPath runtimeFromCliChange}" \
          || fail "CLI-only source change changed the runtime derivation"

        test "${drvPath runtime}" != "${drvPath runtimeFromRuntimeChange}" \
          || fail "runtime-only source change did not change the runtime derivation"
        test "${drvPath cli}" = "${drvPath cliFromRuntimeChange}" \
          || fail "runtime-only source change changed the CLI derivation"

        test "${drvPath runtime}" != "${drvPath runtimeFromModelChange}" \
          || fail "model-only source change did not change the runtime derivation"
        test "${drvPath cli}" = "${drvPath cliFromModelChange}" \
          || fail "model-only source change changed the CLI derivation"

        test "${drvPath cli}" = "${drvPath cliFromTestChildChange}" \
          || fail "test-child source change changed the CLI derivation"
        test "${drvPath runtime}" = "${drvPath runtimeFromTestChildChange}" \
          || fail "test-child source change changed the runtime derivation"

        test "${drvPath runtime}" != "${drvPath debugRuntime}" \
          || fail "release and debug runtime derivations share an identity"
      ''
    else
      ''
        echo "package-boundaries: source matrix runs only on its target system" >&2
      '';
in
assert builtins.hasAttr "nixfied-cli" publicPackages;
assert builtins.hasAttr "nixfied-runtime" publicPackages;
assert !(builtins.hasAttr "nixfied-runtime-debug" publicPackages);
assert !(builtins.hasAttr "nixfied-test-child" publicPackages);
pkgs.stdenv.mkDerivation {
  name = "nixfied-package-boundaries";
  src = sourceStaging;
  cargoDeps = pkgs.rustPlatform.importCargoLock {
    lockFile = ../../runtime/Cargo.lock;
  };
  nativeBuildInputs = [
    rustToolchain
    pkgs.rustPlatform.cargoSetupHook
    pkgs.findutils
    pkgs.jq
  ];
  buildPhase = ''
  set -euo pipefail

  fail() {
    echo "package-boundaries: $*" >&2
    exit 1
  }

  check_root() {
    local root="$1"
    local expected="$2"
    local path
    local metadata
    while IFS= read -r path; do
      case "$expected:$path" in
        cli:Cargo.toml|cli:Cargo.lock|cli:crates/nixfied-cli/*) ;;
        runtime:Cargo.toml|runtime:Cargo.lock|runtime:crates/nixfied-model/*|runtime:crates/nixfied-runtime/*) ;;
        test-child:Cargo.toml|test-child:Cargo.lock|test-child:crates/nixfied-test-child/*) ;;
        *) fail "$expected source root contains unexpected file $path" ;;
      esac
    done < <(find "$root" -type f -printf '%P\n' | sort)

    test -f "$root/Cargo.toml" || fail "$expected source root has no Cargo.toml"
    test -f "$root/Cargo.lock" || fail "$expected source root has no Cargo.lock"
    metadata="$(mktemp)"
    cargo metadata --manifest-path "$root/Cargo.toml" --locked --offline --format-version 1 \
      >"$metadata" \
      || fail "$expected source root failed cargo metadata --locked --offline"
    case "$expected" in
      cli)
        jq -e '([.packages[] as $package | select(.workspace_members | index($package.id)) | $package.name] | sort) == ["nixfied-cli"]' "$metadata" >/dev/null
        jq -e '([.packages[] | select(.source != null) | .name] | sort) == []' "$metadata" >/dev/null
        ;;
      runtime)
        jq -e '([.packages[] as $package | select(.workspace_members | index($package.id)) | $package.name] | sort) == ["nixfied-model", "nixfied-runtime"]' "$metadata" >/dev/null
        jq -e '([.packages[] | select(.source != null) | .name] | length) == 38' "$metadata" >/dev/null
        ;;
      test-child)
        jq -e '([.packages[] as $package | select(.workspace_members | index($package.id)) | $package.name] | sort) == ["nixfied-test-child"]' "$metadata" >/dev/null
        jq -e '([.packages[] | select(.source != null) | .name] | sort) == ["libc"]' "$metadata" >/dev/null
        ;;
    esac || fail "$expected source root exposed the wrong Cargo members"
    rm -f "$metadata"
  }

  check_root "$PWD/cli" cli
  check_root "$PWD/runtime" runtime
  check_root "$PWD/test-child" test-child
  test -f "$PWD/runtime/crates/nixfied-model/capability.txt" \
    || fail "runtime source root omitted nixfied-model/capability.txt"

  ${sourceBoundaryAssertions}

  cli_files="$(find "${publicPackages.nixfied-cli}" -mindepth 1 \( -type f -o -type l \) -printf '%P\n' | sort)"
  test "$cli_files" = "bin/nixfied" \
    || fail "CLI output contains unexpected files: $cli_files"
  runtime_files="$(find "${publicPackages.nixfied-runtime}" -mindepth 1 \( -type f -o -type l \) -printf '%P\n' | sort)"
  test "$runtime_files" = "bin/nixfied-runtime" \
    || fail "runtime output contains unexpected files: $runtime_files"

  '';
  installPhase = ''
    touch "$out"
  '';
}
