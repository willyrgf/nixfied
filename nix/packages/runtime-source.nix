# Build a real package-specific Cargo workspace root from the canonical runtime
# workspace. The filtered path is the package's source identity: unrelated crate
# files are not inputs to the product derivation.
{
  pkgs,
  source,
  package,
}:
let
  lib = pkgs.lib;
  membersByPackage = {
    nixfied-cli = [ "crates/nixfied-cli" ];
    nixfied-runtime = [
      "crates/nixfied-model"
      "crates/nixfied-runtime"
    ];
    nixfied-test-child = [ "crates/nixfied-test-child" ];
  };
  selectedMembers =
    if builtins.hasAttr package membersByPackage then
      membersByPackage.${package}
    else
      throw "nixfied source filtering: unsupported package ${package}";
  sourcePath = toString source;
  canonicalManifest = builtins.readFile ../../runtime/Cargo.toml;
  canonicalMembers = ''
members = [
  "crates/nixfied-cli",
  "crates/nixfied-model",
  "crates/nixfied-runtime",
  "crates/nixfied-test-child",
]
'';
  replacementMembers = ''
members = [
${lib.concatMapStringsSep "\n" (member: "  \"${member}\",") selectedMembers}
]
'';
  rewrittenManifest =
    if builtins.length (lib.splitString canonicalMembers canonicalManifest) != 2 then
      throw "nixfied source filtering: canonical members block must occur exactly once in ${source}/Cargo.toml"
    else
      lib.replaceStrings [ canonicalMembers ] [ replacementMembers ] canonicalManifest;
  relativePath = path:
    let
      value = toString path;
    in
    if value == sourcePath then "" else lib.removePrefix "${sourcePath}/" value;
  isSelected = relative:
    relative == ""
    || relative == "Cargo.toml"
    || relative == "Cargo.lock"
    || lib.any (member: relative == member || lib.hasPrefix "${member}/" relative) selectedMembers;
  isSelectedDirectory = relative:
    relative == ""
    || lib.any (
      member:
      relative == member
      || lib.hasPrefix "${relative}/" member
      || lib.hasPrefix "${member}/" relative
    ) selectedMembers;
  filteredSource = builtins.path {
    name = "nixfied-${package}-source-filter";
    path = source;
    filter = path: type:
      let
        relative = relativePath path;
      in
      if type == "directory" then isSelectedDirectory relative else isSelected relative;
  };
  manifestFile = builtins.toFile "nixfied-${package}-Cargo.toml" rewrittenManifest;
  root = pkgs.runCommand "nixfied-${package}-source" { } ''
    mkdir -p "$out"
    cp -R --no-preserve=mode,ownership ${filteredSource}/. "$out/"
    cp ${manifestFile} "$out/Cargo.toml"
  '';
in
{
  inherit root selectedMembers;
  members = selectedMembers;
}
