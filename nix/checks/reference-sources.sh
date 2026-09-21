# Gate-only fixtures copy the current supplying source, never historical work.
local work source current_system variant project expected_source executable
work=$(mktemp -d)
source=$(nix flake metadata --no-write-lock-file --json "$checkout" | jq -er .path)
current_system=$(nix eval --impure --raw --expr builtins.currentSystem)
for variant in one two; do
  cp -R "$source" "$work/framework-$variant"
  chmod -R u+w "$work/framework-$variant"
  printf '\nRevision binding fixture: source-%s\n' "$variant" >> "$work/framework-$variant/docs/GUIDE.md"
  project="$work/project-$variant"
  mkdir "$project"
  cat > "$project/flake.nix" <<FIXTURE
{
  inputs.nixfied.url = "path:$work/framework-$variant";
  outputs = { nixfied, ... }: {
    apps.$current_system = nixfied.lib.$current_system.projectApps ./nixfied.nix;
    supplyingSource = nixfied.outPath;
  };
}
FIXTURE
  cat > "$project/nixfied.nix" <<'MODULE'
{ ... }: {
  nixfied.project.projectId = throw "docs forced invalid project identity";
  nixfied.project.name = throw "docs forced invalid project name";
  nixfied.closures.unused.package = throw "docs forced project executable";
  nixfied.surface.verbs = { };
}
MODULE
  nix flake lock "$project" >&2 || fail 'reference: downstream lock failed'
  expected_source=$(nix eval --raw "$project#supplyingSource")
  executable=$(nix eval --raw "$project#apps.$current_system.docs.program")
  nix run --no-write-lock-file "$project#docs" -- topic authoring > "$work/topic-$variant"
  grep -Fq "Revision binding fixture: source-$variant" "$work/topic-$variant" \
    || fail 'reference: downstream content did not follow its pin'
  nix run --no-write-lock-file "$project#docs" -- source > "$work/source-$variant"
  grep -Fxq "path: $expected_source" "$work/source-$variant" \
    || fail 'reference: provenance did not match the supplying input'
  if grep -Eq '^revision:' "$work/source-$variant"; then
    fail 'reference: a path source claimed a committed revision'
  fi
  # Invoke the already realised executable from another project: no current
  # directory, Nix registry or runtime state lookup may redirect its content.
  (cd "$work"; NIXFIED_STATE_DIR="$work/no-state" "$executable" source) > "$work/direct-$variant"
  cmp "$work/source-$variant" "$work/direct-$variant" \
    || fail 'reference: realised docs changed source with its caller'
  test ! -e "$work/no-state" || fail 'reference: docs materialised runtime state'
  # Help retains its native final-app evaluation boundary. Supply valid model
  # metadata for that separate discovery proof after the poisoned docs queries.
  cat > "$project/nixfied.nix" <<'MODULE'
{ adapters, ... }: {
  imports = [ adapters.synthetic ];
  nixfied.project.projectId = "reference-fixture";
  nixfied.project.name = "Reference fixture";
}
MODULE
  (cd "$project"; nix run --no-write-lock-file .#help) > "$work/help-$variant"
  grep -Eq '^  docs +' "$work/help-$variant" || fail 'reference: project help omitted docs'
done
if cmp -s "$work/source-one" "$work/source-two"; then
  fail 'reference: distinct supplying sources produced identical provenance'
fi
rm -rf "$work"
