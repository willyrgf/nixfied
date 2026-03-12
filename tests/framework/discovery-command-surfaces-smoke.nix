{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  loggingPrelude = shellHelpers.loggingPrelude;

  compiledDiscovery = import ../../nixfied/framework/runtime/helpers/discovery.nix {
    inherit pkgs loggingPrelude;
    project.discovery = {
      enable = true;
      strict = true;
    };
    commandSurfaces = [
      {
        name = "compiled-alpha";
        owner_file = "custom/alpha.nix";
      }
      {
        name = "compiled-beta";
        owner_file = "";
      }
      {
        name = "compiled-alpha";
        owner_file = "custom/alpha.nix";
      }
      {
        name = "";
        owner_file = "ignored/empty-name.nix";
      }
    ];
    featureInventory = {
      "runtime.fixture.alpha" = {
        id = "runtime.fixture.alpha";
        kind = "runtime";
        summary = "Runtime fixture alpha";
        status = "stable";
        coverageRequired = true;
      };
      "task.fixture.beta" = {
        id = "task.fixture.beta";
        kind = "task";
        summary = "Task fixture beta";
        status = "stable";
        coverageRequired = false;
      };
    };
  };

  fallbackDiscovery = import ../../nixfied/framework/runtime/helpers/discovery.nix {
    inherit pkgs loggingPrelude;
    project.discovery = {
      enable = true;
      strict = true;
    };
  };
in
pkgs.runCommand "discovery-command-surfaces-smoke" { } ''
    set -euo pipefail
    ${shellHelpers.shellPrelude}

    COMPILED_BIN="${compiledDiscovery.tool}/bin/nixfied-discovery-index"
    FALLBACK_BIN="${fallbackDiscovery.tool}/bin/nixfied-discovery-index"
    JQ_BIN="${pkgs.jq}/bin/jq"

    require_jq() {
      local path="$1"
      local expr="$2"
      local description="$3"
      if ! "$JQ_BIN" -e "$expr" "$path" >/dev/null; then
        echo "--- $path"
        cat "$path"
        fail "$description"
      fi
    }

    write_fixture_root() {
      local root="$1"
      mkdir -p "$root"
      cat > "$root/flake.nix" <<'EOF'
  {
    description = "fixture";
  }
  EOF
      cat > "$root/README.md" <<'EOF'
  # fixture
  EOF
    }

    compiled_root="$TMPDIR/compiled-root"
    write_fixture_root "$compiled_root"
    mkdir -p "$compiled_root/nixfied/project"
    cat > "$compiled_root/nixfied/project/module.nix" <<'EOF'
  { }:
  {
    commands.bogus = true;
    commands.shadow = true;
  }
  EOF

    "$COMPILED_BIN" --refresh --root "$compiled_root" > "$TMPDIR/compiled-refresh.out" 2>&1 || {
      cat "$TMPDIR/compiled-refresh.out"
      fail "compiled discovery refresh failed"
    }

    compiled_index="$compiled_root/docs/repo-index.json"
    compiled_map="$compiled_root/docs/repo-map.md"

    require_jq "$compiled_index" '.command_surfaces | length == 2' "compiled command surfaces were not normalized"
    require_jq "$compiled_index" '.command_surfaces[] | select(.name == "compiled-alpha" and .owner_file == "custom/alpha.nix")' "compiled-alpha surface missing"
    require_jq "$compiled_index" '.command_surfaces[] | select(.name == "compiled-beta" and .owner_file == "nixfied/project/module.nix")' "compiled-beta default owner_file missing"
    require_jq "$compiled_index" '.features | length == 2' "compiled feature inventory was not normalized"
    require_jq "$compiled_index" '.features[] | select(.id == "runtime.fixture.alpha" and .kind == "runtime" and .coverage_required == true)' "runtime feature missing"
    require_jq "$compiled_index" '.features[] | select(.id == "task.fixture.beta" and .kind == "task" and .coverage_required == false)' "task feature missing"
    require_not_contains "$compiled_index" '"bogus"'
    require_not_contains "$compiled_index" '"shadow"'
    require_contains "$compiled_map" '- `compiled-alpha` from `custom/alpha.nix`'
    require_contains "$compiled_map" '- `compiled-beta` from `nixfied/project/module.nix`'
    require_contains "$compiled_map" '## Features'
    require_contains "$compiled_map" '- `runtime.fixture.alpha` [runtime] - Runtime fixture alpha (coverage required)'
    require_contains "$compiled_map" '- `task.fixture.beta` [task] - Task fixture beta'
    require_not_contains "$compiled_map" '`bogus`'

    fallback_root="$TMPDIR/fallback-root"
    write_fixture_root "$fallback_root"
    mkdir -p "$fallback_root/nixfied/project"
    cat > "$fallback_root/nixfied/project/dev.nix" <<'EOF'
  { }:
  {
    commands.dev = true;
  }
  EOF

    "$FALLBACK_BIN" --refresh --root "$fallback_root" > "$TMPDIR/fallback-refresh.out" 2>&1 || {
      cat "$TMPDIR/fallback-refresh.out"
      fail "fallback discovery refresh failed"
    }

    fallback_index="$fallback_root/docs/repo-index.json"
    fallback_map="$fallback_root/docs/repo-map.md"

    require_jq "$fallback_index" '.command_surfaces[] | select(.name == "dev" and .owner_file == "nixfied/project/dev.nix")' "fallback grep path did not discover dev command"
    require_jq "$fallback_index" '.features | length == 0' "fallback features should be empty without compiled inventory"
    require_contains "$fallback_map" '- `dev` from `nixfied/project/dev.nix`'
    require_contains "$fallback_map" '## Features'
    require_contains "$fallback_map" '- (none detected)'

    echo "OK: discovery projections prefer compiled command and feature data and preserve fallback scanning" > "$out"
''
