# Nixfied

A generic, Nix-first framework for development, testing, and production workflows.
All behavior is configured in `nix/project.nix`.

## Quick Start

```bash
nix run .#help
nix run .#dev
nix run .#test
nix run .#build
nix run .#ci
nix run .#check
```

By default, the commands are placeholders and will exit with a message until you
replace them in `nix/project.nix`.

## Configuration

Everything is driven by `nix/project.nix`:

- Project metadata (name/id)
- Environment variables (`PROJECT_ENV`, `NIX_ENV`)
- Port roles and offsets
- Command scripts and descriptions
- Optional modules (PostgreSQL, Nginx, Playwright)
- Supervisor services

### Minimal command configuration

```nix
commands = {
  dev = {
    description = "Start dev server";
    env = { PROJECT_ENV = "dev"; };
    useDeps = true;
    script = ''
      echo "hello from dev"
    '';
  };
  test = {
    description = "Run tests";
    env = { PROJECT_ENV = "test"; };
    useDeps = true;
    script = ''
      echo "hello from test"
    '';
  };
  build = {
    description = "Build artifacts";
    env = { PROJECT_ENV = "prod"; };
    useDeps = true;
    script = ''
      echo "hello from build"
    '';
  };
  ci = {
    description = "Run CI pipeline";
    env = { PROJECT_ENV = "test"; };
    useDeps = true;
    script = ''
      echo "hello from ci"
    '';
  };
  check = {
    description = "Run quality checks";
    env = { };
    useDeps = true;
    script = ''
      echo "hello from check"
    '';
  };
};
```

### Dependency installation hook

If your commands need dependencies, set `install.deps`:

```nix
install.deps = ''
  # Example: language/package manager install
  echo "installing dependencies"
  # e.g. npm install, bun install, pip install -r requirements.txt
'';
```

## Environment and Slots

- `PROJECT_ENV` selects the environment (e.g., dev/test/prod).
- `NIX_ENV` selects the slot (0-9).

Ports are computed from:

```
computed_port = base_port + slot + env_offset
```

You define base ports and environment offsets in `nix/project.nix`.

## Supervisor (Production Orchestration)

Supervisor is first-class and configured via `project.supervisor.services`.
Each service defines a command, env, dependencies, and readiness checks.

Example:

```nix
supervisor = {
  enable = true;
  services = {
    app = {
      command = ''
        echo "start app"
      '';
      workingDir = ".";
      env = { PORT = "3000"; };
      readiness = {
        type = "http";
        host = "127.0.0.1";
        port = "3000";
        path = "/";
      };
    };
  };
};
```

## Optional Modules

Disabled by default. Enable them in `nix/project.nix`:

- PostgreSQL
- Nginx
- Playwright

Each module exposes helpers and scripts you can integrate into your commands.

## Sanity Checks

```bash
nix flake show
nix flake check --no-build
```
