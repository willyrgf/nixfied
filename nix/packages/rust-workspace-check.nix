# Hermetic source gate for the runtime workspace: format + lint + typecheck.
#
# Run from the pinned toolchain with vendored deps so `nix flake check` proves the
# Rust source is `rustfmt`-clean, `clippy`-clean (`-D warnings`), and type-checks —
# with no host toolchain and no network. The white-box `cargo test` floor is
# intentionally *not* here (it binds ports / spawns process groups); that runs via
# `.#test` / `.#ci` outside the sandbox.
{ pkgs }:
let
  rustToolchain = pkgs.rust-bin.stable."1.96.0".minimal.override {
    extensions = [
      "clippy"
      "rustfmt"
    ];
  };
in
pkgs.stdenv.mkDerivation {
  name = "nixfied-rust-workspace-check";
  src = ../../runtime;
  cargoDeps = pkgs.rustPlatform.importCargoLock {
    lockFile = ../../runtime/Cargo.lock;
  };
  nativeBuildInputs = [
    rustToolchain
    pkgs.rustPlatform.cargoSetupHook
  ];
  buildPhase = ''
    runHook preBuild
    cargo fmt --all -- --check
    # `clippy` runs the full rustc front end, so `--all-targets -D warnings` also
    # type-checks every target — a separate `cargo check` would just recompile the
    # workspace a second time.
    cargo clippy --all-targets -- -D warnings
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    mkdir -p "$out"
    runHook postInstall
  '';
  doCheck = false;
}
