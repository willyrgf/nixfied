# Nixfied

A generic, Nix-first framework for development, testing, CI, and production.
All behavior is configured in `nix/project/` and is safe by default (no-op
commands that exit 0).

## Contents

- Quick start
- Install into an existing repo
- Repository layout
- Configuration model
- Command definitions
- Execution environment
- Slots, environments, and ports
- Framework helpers (shell)
- CI pipeline DSL
- Optional modules (Postgres, Nginx, Playwright)
- Supervisor (process-compose)
- Dev shell and packages
- Sanity checks

## Quick start

```bash
nix run .#help
nix run .#dev
nix run .#test
nix run .#build
nix run .#ci
nix run .#check
```

Template defaults are safe no-ops: `dev`, `test`, `build`, and `check` print a
placeholder and exit 0. The CI pipeline is enabled and runs placeholder steps.
Replace each command in its file under `nix/project/`.

## Install into an existing repo

From your target repository:

```bash
cd my-app
nix run github:willyrgf/nixfied#install
```

Safety behavior:
- If the repo name does not end with `_nixified`, the installer copies the repo
  to `<repo>_nixified` and re-runs itself there.
- It refuses to install unless the repo name ends with `_nixified`.
- It installs only `flake.nix`, `flake.lock`, and `nix/`.

Force overwrite:

```bash
nix run github:willyrgf/nixfied#install -- --force
# or
NIXFIED_INSTALL_FORCE=1 nix run github:willyrgf/nixfied#install
```

Filter which project files are installed (conf is always included):

```bash
nix run github:willyrgf/nixfied#install -- --filter=conf,test,ci
```

## Repository layout

```
flake.nix
nix/
  apps/
    core.nix        # dev/test/build/check/help apps
    install.nix     # installer app
  project/
    conf.nix        # base configuration
    dev.nix         # dev command
    test.nix        # test command
    prod.nix        # build/prod command(s)
    quality.nix     # check command
    ci.nix          # CI command + pipeline DSL
    default.nix     # merges the files above
  ci.nix            # pipeline runner implementation
  lib.nix           # helper functions and app builder
  slots.nix         # slot/env/port logic
  hooks.nix         # exported hook env vars
  postgres.nix      # optional postgres module
  nginx.nix         # optional nginx module
  playwright.nix    # optional playwright module
  supervisor.nix    # process-compose config generator
  devshell.nix      # nix develop shell
```

## Configuration model

All project configuration lives in `nix/project/`.
`nix/project/default.nix` merges the files below via `recursiveUpdate`.

### `nix/project/conf.nix`

Base configuration and module toggles.

```nix
project = {
  name = "Nixfied Project";
  id = "nixfied-project";
  description = "Reusable Nix development framework";
  envVar = "PROJECT_ENV";  # env name (dev/test/prod)
  slotVar = "NIX_ENV";     # slot number (0-9)
};

envs = {
  prod = { offset = 0; };
  dev  = { offset = 10; };
  test = { offset = 20; };
};

ports = {
  backend = 3000;
  frontend = 3100;
  http = 8080;
  https = 8443;
  postgres = 5432;
};

directories.base = "${XDG_DATA_HOME:-$HOME/.local/share}/${project.id}";

tooling.runtimePackages = [ pkgs.coreutils pkgs.gnused ];
# tooling.devShellPackages = [ pkgs.nodejs_20 ];
# tooling.devShellHook = ''echo "dev shell ready"'';

install.deps = ''
  # language/package manager install
  # e.g. npm install, bun install, pip install -r requirements.txt
'';

supervisor.enable = true;
supervisor.services = { };

modules.postgres.enable = false;
modules.nginx.enable = false;
modules.playwright.enable = false;

packages = { };
```

### `nix/project/*.nix` command files

Each file exports a `commands` attrset. You can add new commands anywhere as
long as they are merged in `nix/project/default.nix`.

Command schema:

```nix
commands.<name> = {
  description = "Shown in nix run .#help";
  env = { PROJECT_ENV = "dev"; };
  useDeps = true;  # runs install.deps before the script
  script = ''
    echo "hello"
  '';
};
```

Files by convention:
- `dev.nix` -> `dev`
- `test.nix` -> `test`
- `prod.nix` -> `build`
- `quality.nix` -> `check`
- `ci.nix` -> `ci`

## Execution environment

Every command is wrapped by `nix/lib.nix` and gets:
- `COMMAND_NAME` set to the command name.
- `.env` loaded if present (does not override existing env vars).
- `tooling.runtimePackages` added to `PATH`.
- `install.deps` (if `useDeps = true`).
- Framework helper functions (see below).
- Hook environment variables from `nix/hooks.nix`.

## Slots, environments, and ports

- `PROJECT_ENV` selects the environment (`dev`, `test`, `prod`).
- `NIX_ENV` selects the slot (0-9).

Ports are computed as:

```
computed_port = base_port + slot + env_offset
```

`nix/slots.nix` exposes helper scripts:
- `SLOT_INFO` prints `SLOT`, `ENV`, `BASE_DIR`, `LOG_DIR`, `RUN_DIR`,
  `CONFIG_DIR`, `STATE_DIR`, and all computed ports.
- `REQUIRE_SLOT_ENV` validates env/slot, prints values, and prompts in TTY
  if `PROJECT_ENV` or `NIX_ENV` were not explicitly set.

Example usage:

```bash
eval "$(${SLOT_INFO})"
echo "Backend port: $BACKEND_PORT"
```

## Framework helpers (shell)

Every command sources a helper script generated by `nix/lib.nix`.

- `require_env VAR [message]`
  - Fail if `VAR` is missing or empty.
- `skip_if_missing VAR [reason]`
  - Return 1 if `VAR` is missing so callers can skip work.
- `wait_http URL [timeout] [interval]`
  - Poll a URL until it responds or timeout expires.
- `wait_port PORT [timeout] [interval]`
  - Wait for a TCP port to listen (uses `lsof` or `nc`).
- `log_capture LOGFILE -- <command...>`
  - Capture stdout/stderr to a file (`LOG_TEE=1` also streams to stdout).
- `summary_parse LOGFILE DURATION EXIT_CODE`
  - Print a compact run summary (used by CI `--summary`).
- `with_cleanup CMD`
  - Register a cleanup command (runs on EXIT/INT/TERM in LIFO order).
- `start_service NAME [opts] -- <command...>`
  - Run a background service with optional logging and readiness checks.
- `stop_service PID [name]`
  - Stop a background service.
- `with_service NAME [start opts] -- <start command...> --run <command...>`
  - Start a service, then run a command; cleanup is automatic.
- `run_hook ENV_VAR [args...]`
  - Execute the command stored in `ENV_VAR`.
- `artifact_dir`
  - Return the CI artifacts directory.
- `artifact_path NAME`
  - Return a full path inside the artifacts directory.

Service example:

```bash
PID=$(start_service backend --log /tmp/backend.log --wait-port 3000 -- ./start-backend)
./run-tests
stop_service "$PID" backend
```

## Nix helper functions (lib)

`nix/lib.nix` also exposes Nix-level helpers you can reuse when wiring custom
apps:

- `mkApp` / `mkAppWithDeps` - wrap a shell script as a flake app with env, deps,
  hooks, and helper functions preloaded.
- `withTiming` - wrap a script to print a duration footer.
- `mkParallelRunner` - generate a script that runs multiple commands in
  parallel and collates output.
- `mkPortCleanup` / `mkPortConflictChecker` - generate scripts to kill or check
  port listeners.
- `mkSignalHandler` / `mkProcessManager` - generate scripts for clean shutdown
  and process supervision.
- `summaryParser`, `helpersScript`, `loadEnv` - internal helpers used by `mkApp`.

To use them, import `nix/lib.nix` in your Nix wiring and call the functions
directly.

## CI pipeline DSL

The CI runner is enabled by default and defined in `nix/project/ci.nix`.

Top-level config:

```nix
ci = {
  enable = true;
  defaultMode = "basic";
  env = { PROJECT_ENV = "test"; };
  useDeps = true;
  setup = "";      # runs once before steps
  teardown = "";   # runs once after steps
  artifacts = {
    dir = "/tmp/ci-artifacts";
    keepOnFailure = true;
    keepOnSuccess = false;
  };
  modes = {
    basic = { steps = [ "quality" "tests" ]; };
    app = { steps = [ "quality" "tests" "system" ]; };
  };
  steps = {
    quality = {
      description = "Quality checks";
      run = ''
        LOGFILE=$(artifact_path "quality.log")
        log_capture "$LOGFILE" -- ./lint
      '';
    };
    system = {
      description = "System tests";
      skipIfMissing = [ "API_KEY" ];
      requires = [ "nginx" ];
      when = "[ \"$PROJECT_ENV\" = test ]";
      cleanup = ''
        echo "cleanup after step"
      '';
      env = { FOO = "bar"; };
      run = ''
        ./run-system-tests
      '';
    };
  };
};
```

Step fields:
- `description` (string)
- `run` (shell script)
- `when` (shell condition, optional)
- `cleanup` (shell script, runs after `run` even on failure)
- `env` (attrset of env vars)
- `skipIfMissing` (list of required env vars)
- `requires` (list of module names, e.g., `postgres`, `nginx`)

CLI usage:

```bash
nix run .#ci                 # default mode
nix run .#ci -- --summary    # summary output
nix run .#ci -- --mode app   # select mode
nix run .#ci -- --app        # shorthand for mode "app"
```

CI environment variables available inside steps:
- `CI_MODE`
- `CI_SUMMARY`
- `CI_ARTIFACTS_DIR`
- `CI_KEEP_ARTIFACTS_ON_FAILURE`
- `CI_KEEP_ARTIFACTS_ON_SUCCESS`

Artifacts:
- Stored in `ci.artifacts.dir` and also in `CI_ARTIFACTS_DIR`.
- Removed automatically unless `keepOnFailure`/`keepOnSuccess` are true.

## Optional modules

Enable modules in `nix/project/conf.nix`.

### Postgres

Config:

```nix
modules.postgres = {
  enable = true;
  database = "app";
  testDatabase = "app_test";
  extensions = [ "pgcrypto" ];
  portKey = "postgres";  # key in ports
  dataDirName = "postgres";
  extraConfig = "";
};
```

Hooks (exported env vars):
- `POSTGRES_INIT`
- `POSTGRES_START`
- `POSTGRES_STOP`
- `POSTGRES_SETUP_DB`
- `POSTGRES_FULL_START`
- `POSTGRES_FULL_START_TEST`

Example:

```bash
run_hook POSTGRES_FULL_START
run_hook POSTGRES_SETUP_DB
```

### Nginx

Config:

```nix
modules.nginx = {
  enable = true;
  portKeyHttp = "http";
  portKeyHttps = "https";
  dataDirName = "nginx";
};
```

Hooks:
- `NGINX_INIT`
- `NGINX_START`
- `NGINX_STOP`
- `NGINX_SITE_PROXY`
- `NGINX_SITE_STATIC`

Example:

```bash
run_hook NGINX_INIT
run_hook NGINX_SITE_PROXY example.localhost 127.0.0.1 3000
run_hook NGINX_START
```

### Playwright

Config:

```nix
modules.playwright = {
  enable = true;
  browsersPath = "/path/to/browsers"; # optional
};
```

The module provides an env setup script (`playwright.envSetup`) at the Nix
level. If you want it inside command scripts, export it via `nix/hooks.nix` or
inline it when wiring a custom app.

## Supervisor (process-compose)

Supervisor is intended for production orchestration. Configure services in
`nix/project/conf.nix`:

```nix
supervisor = {
  enable = true;
  services = {
    app = {
      command = ''./start-app'';
      workingDir = ".";
      env = { PORT = "3000"; };
      readiness = {
        type = "http";
        host = "127.0.0.1";
        port = "3000";
        path = "/health";
      };
    };
  };
};
```

The `nix/supervisor.nix` module generates a process-compose YAML and provides
scripts to `start`, `stop`, and `status`. If you want to use these from your
command scripts, import the module in your Nix code and run the script path
(e.g., `toString supervisor.start`).

## Dev shell and packages

- `nix develop` uses `tooling.devShellPackages` and `tooling.devShellHook`.
- `project.packages` is exposed via `flake.packages` for custom outputs.

## Sanity checks

```bash
nix flake show
nix flake check --no-build
```
